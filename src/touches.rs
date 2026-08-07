//! Touch memory: a persistent ring of recent code edits with rationale.
//!
//! Storage is `<root>/.gigagraph/touches.jsonl`, one JSON object per line
//! (`{ts, files, why, agent}`), oldest first. Ring semantics are enforced on
//! every write: the most recent [`MAX_GLOBAL`] entries are kept globally, and
//! no single file may be mentioned by more than [`MAX_PER_FILE`] kept entries
//! (oldest extras are dropped). The file is rewritten atomically (tmp +
//! rename) under an exclusive advisory lock (`touches.lock`), because both
//! the MCP server and the editor hook may write concurrently.
//!
//! This is agent-/hook-reported history — `git log` stays authoritative; the
//! ring adds the WHY and covers uncommitted work.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Most recent entries kept globally.
pub const MAX_GLOBAL: usize = 250;
/// Most entries allowed to mention any single file.
pub const MAX_PER_FILE: usize = 10;

/// One recorded edit: when, which files, why, and by whom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Touch {
    /// Unix seconds.
    pub ts: u64,
    /// Repo-relative paths.
    pub files: Vec<String>,
    /// One-line rationale ("(auto) edited via hook" for mechanical entries).
    pub why: String,
    /// Who recorded it ("hook", an agent name, or "unknown").
    pub agent: String,
}

/// Result of a successful [`record_touch`].
pub struct RecordOutcome {
    pub entry: Touch,
    /// Entries in the ring after trimming.
    pub total_entries: usize,
    /// Kept-entry count per file of the new entry (post-trim).
    pub file_counts: Vec<(String, usize)>,
}

fn touches_dir(root: &Path) -> PathBuf {
    root.join(".gigagraph")
}

fn touches_path(root: &Path) -> PathBuf {
    touches_dir(root).join("touches.jsonl")
}

fn lock_path(root: &Path) -> PathBuf {
    touches_dir(root).join("touches.lock")
}

/// Normalizes a user-supplied path (repo-relative or absolute) to a
/// repo-relative, forward-slash path. Absolute paths outside the repo are
/// kept absolute rather than rejected — better a lossy record than none.
fn normalize_rel(root: &Path, raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    let p = PathBuf::from(&trimmed);
    if !p.is_absolute() {
        return trimmed.trim_start_matches("./").to_string();
    }
    // Canonicalize when possible so symlinked prefixes (/var -> /private/var)
    // still strip; fall back to the lexical path for files that are gone.
    let abs = p.canonicalize().unwrap_or(p);
    let roots = [root.canonicalize().ok(), Some(root.to_path_buf())];
    for r in roots.iter().flatten() {
        if let Ok(rel) = abs.strip_prefix(r) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }
    abs.to_string_lossy().replace('\\', "/")
}

/// Does `entry_file` (repo-relative) refer to the queried `file`? Exact
/// match, or unique-suffix convenience (`api.rs` matches `src/api.rs`).
fn mentions(entry_file: &str, file: &str) -> bool {
    entry_file == file || entry_file.ends_with(&format!("/{file}"))
}

// ---- locking ----

/// Exclusive advisory lock via `create_new` on `touches.lock`; the file is
/// removed on drop. Spins with short sleeps; a lock older than
/// `STALE_AFTER` is treated as leftover from a crashed writer and broken.
struct TouchLock {
    path: PathBuf,
}

const STALE_AFTER: Duration = Duration::from_secs(10);
const LOCK_RETRY: Duration = Duration::from_millis(15);
const LOCK_ATTEMPTS: usize = 200; // ~3s total

impl TouchLock {
    fn acquire(root: &Path) -> Result<TouchLock> {
        let path = lock_path(root);
        for _ in 0..LOCK_ATTEMPTS {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    let _ = writeln!(f, "{}", std::process::id());
                    return Ok(TouchLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Break stale locks left by crashed writers.
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|m| m.elapsed().ok())
                        .is_some_and(|age| age > STALE_AFTER);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    std::thread::sleep(LOCK_RETRY);
                }
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("cannot create lock {}", path.display()));
                }
            }
        }
        bail!(
            "could not acquire touch lock {} (another writer is holding it)",
            path.display()
        )
    }
}

impl Drop for TouchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// ---- read / write ----

/// All entries currently in the ring, oldest first. Malformed lines are
/// skipped (the file is shared with external writers). Missing file = empty.
pub fn read_touches(root: &Path) -> Result<Vec<Touch>> {
    let path = touches_path(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    Ok(raw
        .lines()
        .filter_map(|l| serde_json::from_str::<Touch>(l).ok())
        .collect())
}

/// The last `limit` entries, newest first — all of them, or only those
/// mentioning `file` (repo-relative or unique suffix).
pub fn recent(root: &Path, file: Option<&str>, limit: usize) -> Result<Vec<Touch>> {
    let all = read_touches(root)?;
    let query = file.map(|f| normalize_rel(root, f));
    Ok(all
        .into_iter()
        .rev()
        .filter(|t| match &query {
            Some(q) => t.files.iter().any(|f| mentions(f, q)),
            None => true,
        })
        .take(limit)
        .collect())
}

/// Appends one entry and enforces ring semantics (global + per-file caps),
/// rewriting the file atomically under the advisory lock.
pub fn record_touch(
    root: &Path,
    files: &[String],
    why: &str,
    agent: &str,
) -> Result<RecordOutcome> {
    let why = why.trim();
    if why.is_empty() {
        bail!("`why` must be a non-empty one-line rationale");
    }
    let mut norm: Vec<String> = Vec::new();
    for f in files {
        let n = normalize_rel(root, f);
        if !n.is_empty() && !norm.contains(&n) {
            norm.push(n);
        }
    }
    if norm.is_empty() {
        bail!("`files` must contain at least one non-empty path");
    }
    let agent = agent.trim();
    let agent = if agent.is_empty() { "unknown" } else { agent };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = Touch {
        ts,
        files: norm,
        why: why.to_string(),
        agent: agent.to_string(),
    };

    let dir = touches_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let _lock = TouchLock::acquire(root)?;
    let mut all = read_touches(root)?;
    all.push(entry.clone());
    let kept = trim_ring(all);
    write_atomic(root, &kept)?;

    let file_counts = entry
        .files
        .iter()
        .map(|f| {
            let n = kept
                .iter()
                .filter(|t| t.files.iter().any(|g| g == f))
                .count();
            (f.clone(), n)
        })
        .collect();
    Ok(RecordOutcome {
        entry,
        total_entries: kept.len(),
        file_counts,
    })
}

/// Ring trim: walk newest -> oldest keeping at most [`MAX_GLOBAL`] entries,
/// dropping any entry that would push one of its files past
/// [`MAX_PER_FILE`] mentions. Returns oldest first (file order).
fn trim_ring(entries: Vec<Touch>) -> Vec<Touch> {
    let mut counts: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();
    let mut kept: Vec<Touch> = Vec::new();
    for t in entries.into_iter().rev() {
        if kept.len() >= MAX_GLOBAL {
            break;
        }
        if t.files
            .iter()
            .any(|f| counts.get(f).copied().unwrap_or(0) >= MAX_PER_FILE)
        {
            continue; // oldest extras for some file — drop the whole entry
        }
        for f in &t.files {
            *counts.entry(f.clone()).or_insert(0) += 1;
        }
        kept.push(t);
    }
    kept.reverse();
    kept
}

/// Rewrites the ring atomically: write a tmp file, then rename over the real
/// one (same directory, so the rename is atomic on POSIX filesystems).
fn write_atomic(root: &Path, entries: &[Touch]) -> Result<()> {
    let path = touches_path(root);
    let tmp = touches_dir(root).join(format!("touches.jsonl.tmp.{}", std::process::id()));
    let mut buf = String::new();
    for t in entries {
        buf.push_str(&serde_json::to_string(t)?);
        buf.push('\n');
    }
    std::fs::write(&tmp, buf).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("cannot rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
