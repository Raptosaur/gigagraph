//! Parallel indexing pipeline: walk -> (cached) parse/extract -> graph build
//! -> vectorize. Incremental via a content-hash extraction cache.

use crate::extract::{self, ExtractedFile};
use crate::graph::{GigaGraph, FileInput};
use crate::lang;
use crate::vector::VectorIndex;
use anyhow::{Context, Result};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FILE_BYTES: u64 = 2_000_000;
pub(crate) const CACHE_DIR: &str = ".gigagraph";

/// Extensions with no LangSpec that are still collected, for IaC endpoint
/// scanning. Must go through `collect_files` (not a side channel) so that
/// `tree_fingerprint` invalidates the index when a .tf changes.
const IAC_EXTS: &[&str] = &["tf"];

/// Directories skipped even when not gitignored.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    "out",
    "target",
    "vendor",
    "Pods",
    "DerivedData",
    ".gradle",
    ".idea",
    "__pycache__",
    ".next",
    ".expo",
    ".gigagraph",
];

pub struct Index {
    pub graph: GigaGraph,
    pub vectors: VectorIndex,
    pub stats: IndexStats,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files: u32,
    pub functions: u32,
    pub calls: u32,
    pub resolved_internal: u32,
    pub resolved_external: u32,
    pub unresolved: u32,
    pub external_packages: u32,
    pub parsed_files: u32,
    pub cached_files: u32,
    pub skipped_files: u32,
    /// Candidate files that produced no `FileInfo` — unreadable, an extension
    /// with no `LangSpec`, or a parse the extractor rejected. Capped; the
    /// count above is exact. A file listed here is INVISIBLE to every tool,
    /// so this is the first thing to check when an answer looks incomplete.
    #[serde(default)]
    pub skipped_paths: Vec<String>,
    /// Endpoints whose handler resolved to an indexed function. Compared with
    /// the endpoint total this is the handler-link rate — a health metric for
    /// route detection, not just a count.
    #[serde(default)]
    pub endpoints_with_handler: u32,
    pub elapsed_ms: u64,
    pub functions_by_language: FxHashMap<String, u32>,
    /// Stat-level tree fingerprint at build time; lets a fresh process skip
    /// re-indexing when nothing changed on disk.
    #[serde(default)]
    pub tree_fingerprint: u64,
    /// The optional post-index LSP enrichment pass ran for this build; a
    /// reloaded index does not redo it unless the tree changed.
    #[serde(default)]
    pub lsp_enriched: bool,
    /// Call edges the language server agreed with the static pick on.
    #[serde(default)]
    pub lsp_confirmed_calls: u32,
    /// Call edges the language server re-pointed at a different function.
    #[serde(default)]
    pub lsp_corrected_calls: u32,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    hash: u64,
    extracted: ExtractedFile,
}

#[derive(Serialize, Deserialize, Default)]
struct ExtractionCache {
    entries: FxHashMap<String, CacheEntry>,
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    h.write(bytes);
    h.finish()
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(CACHE_DIR).join("cache.bin")
}

fn index_path(root: &Path) -> PathBuf {
    root.join(CACHE_DIR).join("index.bin")
}

/// Walks `root` collecting indexable source files (relative path, absolute).
fn collect_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut walker = ignore::WalkBuilder::new(root);
    walker
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !(entry.file_type().is_some_and(|t| t.is_dir()) && SKIP_DIRS.contains(&name.as_ref()))
        });

    let mut files = Vec::new();
    for entry in walker.build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if lang::spec_for_ext(&ext).is_none() && !IAC_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with(".min.js") || name.ends_with(".bundle.js") {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        files.push((rel, path.to_path_buf()));
    }
    files
}

/// Cheap staleness probe: hash of (path, mtime, len) for every indexable file.
/// Changes whenever a file is added, removed, or touched — without reading
/// file contents.
pub fn tree_fingerprint(root: &Path) -> u64 {
    let mut files = collect_files(root);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = rustc_hash::FxHasher::default();
    // Salt with the binary version: detection logic lives in this binary, so
    // an upgrade must invalidate persisted indexes even when the tree is
    // unchanged — otherwise new detectors silently serve stale results.
    h.write(env!("CARGO_PKG_VERSION").as_bytes());
    // Dependency manifests feed project-level endpoint evidence but are not
    // walked as source files — hash them so edits invalidate too.
    for manifest in [
        "composer.json",
        "package.json",
        "requirements.txt",
        "requirements-dev.txt",
        "pyproject.toml",
        "Pipfile",
    ] {
        if let Ok(bytes) = std::fs::read(root.join(manifest)) {
            h.write(&bytes);
        }
    }
    for (rel, path) in files {
        h.write(rel.as_bytes());
        if let Ok(meta) = std::fs::metadata(&path) {
            h.write_u64(meta.len());
            if let Ok(m) = meta.modified() {
                if let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) {
                    h.write_u128(d.as_nanos());
                }
            }
        }
    }
    h.finish()
}

pub fn build_index(root: &Path, force: bool) -> Result<Index> {
    let started = Instant::now();
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot resolve root {}", root.display()))?;

    let old_cache: ExtractionCache = if force {
        ExtractionCache::default()
    } else {
        std::fs::read(cache_path(&root))
            .ok()
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_default()
    };

    let fingerprint = tree_fingerprint(&root);
    let files = collect_files(&root);
    let total_candidates = files.len() as u32;

    struct Processed {
        rel: String,
        hash: u64,
        extracted: ExtractedFile,
        reused: bool,
        iac: Vec<crate::iac::IacFinding>,
    }

    let processed: Vec<Processed> = files
        .par_iter()
        .filter_map(|(rel, abs)| {
            let bytes = std::fs::read(abs).ok()?;
            let source = String::from_utf8_lossy(&bytes);
            let hash = hash_bytes(source.as_bytes());
            // IaC findings are re-scanned every build (never cached): the
            // text is already in hand for hashing, and caching them would
            // break the bincode cache format for no savings.
            let iac = crate::iac::scan(rel, &source);
            if let Some(entry) = old_cache.entries.get(rel) {
                if entry.hash == hash {
                    return Some(Processed {
                        rel: rel.clone(),
                        hash,
                        extracted: entry.extracted.clone(),
                        reused: true,
                        iac,
                    });
                }
            }
            let ext = abs.extension()?.to_str()?.to_ascii_lowercase();
            let extracted = match lang::spec_for_ext(&ext) {
                Some(spec) => extract::extract(spec, &source)?,
                // IaC-only files (.tf) have no LangSpec; an empty extraction
                // still yields a FileInfo so IaC endpoints get a valid
                // file_id (api.rs indexes g.files[e.file_id] unguarded).
                None => ExtractedFile {
                    language: crate::types::Lang::Terraform,
                    package: None,
                    imports: Vec::new(),
                    functions: Vec::new(),
                    type_decorations: Vec::new(),
                    consts: Vec::new(),
                    fields: Vec::new(),
                    hierarchy: Vec::new(),
                },
            };
            Some(Processed {
                rel: rel.clone(),
                hash,
                extracted,
                reused: false,
                iac,
            })
        })
        .collect();

    let cached_files = processed.iter().filter(|p| p.reused).count() as u32;
    let parsed_files = processed.len() as u32 - cached_files;

    // Which candidates fell out of the parallel pass above (unreadable, no
    // LangSpec for the extension, extraction refused). Capped: the point is
    // to make the shape of the loss visible, not to dump a build log.
    const SKIPPED_SAMPLE: usize = 50;
    let kept: FxHashSet<&str> = processed.iter().map(|p| p.rel.as_str()).collect();
    let skipped_paths: Vec<String> = files
        .iter()
        .map(|(rel, _)| rel)
        .filter(|rel| !kept.contains(rel.as_str()))
        .take(SKIPPED_SAMPLE)
        .cloned()
        .collect();

    let new_cache = ExtractionCache {
        entries: processed
            .iter()
            .map(|p| {
                (
                    p.rel.clone(),
                    CacheEntry {
                        hash: p.hash,
                        extracted: p.extracted.clone(),
                    },
                )
            })
            .collect(),
    };

    let iac_files: Vec<(String, Vec<crate::iac::IacFinding>)> = processed
        .iter()
        .filter(|p| !p.iac.is_empty())
        .map(|p| (p.rel.clone(), p.iac.clone()))
        .collect();

    let inputs: Vec<FileInput> = processed
        .into_iter()
        .map(|p| FileInput {
            path: p.rel,
            content_hash: p.hash,
            extracted: p.extracted,
        })
        .collect();

    let root_str = root.to_string_lossy().to_string();
    // Project-level endpoint evidence: composer.json dependency names cover
    // script-style files (Silex `src/controllers.php`) that route on an
    // `$app` arriving via `require` with no framework import in sight.
    let manifest_deps = |file: &str, keys: &[&str]| -> String {
        std::fs::read_to_string(root.join(file))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| {
                keys.iter()
                    .filter_map(|k| v.get(k)?.as_object().cloned())
                    .flat_map(|m| m.keys().map(|k| k.to_ascii_lowercase()).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    // package.json dependency names cover convention-driven plugins with no
    // per-file import trace (@fastify/autoload directory routing).
    let mut project_deps = format!(
        "{}\n{}",
        manifest_deps("composer.json", &["require", "require-dev"]),
        manifest_deps("package.json", &["dependencies", "devDependencies"])
    );
    // Python dependency manifests, appended raw (evidence is substring-
    // matched): route modules routinely take the app/router as a parameter
    // with no framework import in sight (aiohttp `setup_routes(app, ...)`).
    for manifest in [
        "requirements.txt",
        "requirements-dev.txt",
        "pyproject.toml",
        "Pipfile",
    ] {
        if let Ok(s) = std::fs::read_to_string(root.join(manifest)) {
            project_deps.push('\n');
            project_deps.push_str(&s.to_ascii_lowercase());
        }
    }
    let (mut graph, features) = GigaGraph::build(root_str, inputs, &project_deps);
    crate::iac::attach(&mut graph, &iac_files);
    let graph = graph;
    let vectors = VectorIndex::build(&features);

    let mut stats = IndexStats {
        files: graph.files.len() as u32,
        functions: graph.functions.len() as u32,
        calls: graph.calls.len() as u32,
        parsed_files,
        cached_files,
        skipped_files: total_candidates - graph.files.len() as u32,
        skipped_paths,
        endpoints_with_handler: graph
            .endpoints
            .endpoints
            .iter()
            .filter(|e| e.handler.is_some())
            .count() as u32,
        external_packages: graph.package_calls.len() as u32,
        elapsed_ms: started.elapsed().as_millis() as u64,
        tree_fingerprint: fingerprint,
        ..Default::default()
    };
    for call in &graph.calls {
        match &call.resolution {
            crate::types::Resolution::Internal { .. } => stats.resolved_internal += 1,
            crate::types::Resolution::External { .. } => stats.resolved_external += 1,
            crate::types::Resolution::Unresolved => stats.unresolved += 1,
        }
    }
    for f in &graph.functions {
        *stats
            .functions_by_language
            .entry(f.language.name().to_string())
            .or_insert(0) += 1;
    }

    let index = Index {
        graph,
        vectors,
        stats,
    };
    if let Err(e) = persist(&root, &index, &new_cache) {
        eprintln!("gigagraph: warning: failed to persist index: {e}");
    }
    Ok(index)
}

fn persist(root: &Path, index: &Index, cache: &ExtractionCache) -> Result<()> {
    let dir = root.join(CACHE_DIR);
    std::fs::create_dir_all(&dir)?;
    // Keep the index out of the user's VCS.
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(&gitignore, "*\n");
    }
    // Write-temp-then-rename: the post-handshake warm thread and the serve
    // loop can both persist concurrently; a reader must never see a torn
    // file (bincode would reject it and force a silent rebuild).
    atomic_write(&cache_path(root), &bincode::serialize(cache)?)?;
    persist_index(root, index)
}

/// Re-persists index.bin only (extraction cache untouched) — the LSP
/// enrichment pass mutates the graph after the build already persisted.
pub fn persist_index(root: &Path, index: &Index) -> Result<()> {
    std::fs::create_dir_all(root.join(CACHE_DIR))?;
    let payload = bincode::serialize(&(&index.graph, &index.vectors, &index.stats))?;
    atomic_write(&index_path(root), &payload)?;
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Loads a previously persisted index, if present and readable.
pub fn load_index(root: &Path) -> Option<Index> {
    let root = root.canonicalize().ok()?;
    let bytes = std::fs::read(index_path(&root)).ok()?;
    let (graph, vectors, stats): (GigaGraph, VectorIndex, IndexStats) =
        bincode::deserialize(&bytes).ok()?;
    Some(Index {
        graph,
        vectors,
        stats,
    })
}
