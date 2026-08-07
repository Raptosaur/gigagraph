//! Optional LSP hybrid-resolution enrichment.
//!
//! The static pipeline stays the always-works baseline (and the sole source
//! for string-edges: endpoints, DI, CDK, RN bridge). When a real language
//! server AND its project config are auto-detected, a post-index pass batches
//! definition queries for exactly the call sites the static resolver marked
//! Heuristic or ambiguous, and rewrites the ones the server settles:
//! callee = confirmed id, confidence = High, `ambiguous_with` cleared, and the
//! call index recorded in `GigaGraph::lsp_confirmed` for provenance.
//!
//! No configuration. Detection failing at any point means the pass silently
//! does nothing; every graph consumer sees the plain static result.
//!
//! Providers: TypeScript via the project-local `tsserver` that ships in
//! every TS project's own `node_modules` (never a global install; its native
//! protocol, not LSP), and Python via pyright (real LSP) from a project-local
//! npm install, a local venv, or PATH. All detected providers run in one
//! pass, each routed its own languages' candidate sites, sharing one
//! deadline. The `LspProvider` trait is deliberately small so more servers
//! can slot in.

use crate::graph::GigaGraph;
use crate::indexer::{self, Index};
use crate::types::{Confidence, Lang, Resolution};
use anyhow::{Context, Result, anyhow};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Total wall-clock budget for one enrichment pass (spawn + project load +
/// every definition query). Hitting it abandons the rest silently.
pub const BATCH_BUDGET: Duration = Duration::from_secs(30);
/// Cap on definition queries per pass; ambiguous sites take priority.
pub const MAX_SITES: usize = 2000;

const LSP_CACHE_FILE: &str = "lsp.bin";

/// One definition lookup: root-relative caller file plus the 1-based
/// line/column of the callee NAME token.
#[derive(Debug, Clone)]
pub struct DefQuery {
    pub file: String,
    pub line: u32,
    pub col: u32,
}

/// Where the server says the definition lives, mapped back to a root-relative
/// path. Lookups landing outside the indexed tree (node_modules, lib.d.ts)
/// yield no answer.
#[derive(Debug, Clone, PartialEq)]
pub struct DefAnswer {
    pub file: String,
    pub line: u32,
}

/// A detected, ready-to-spawn language server. Implementations own their
/// process lifecycle inside `resolve`: spawn once per batch, answer each
/// query (best effort, respecting `deadline`), kill the child before
/// returning. `resolve` must never panic the pass — protocol trouble is an
/// `Err` (abandon silently) or a `None` per query.
pub trait LspProvider {
    fn name(&self) -> &'static str;
    /// Which indexed languages this provider can answer for.
    fn handles(&self, lang: Lang) -> bool;
    /// One entry per query, in order. `None` = server had no usable answer.
    fn resolve(
        &mut self,
        root: &Path,
        queries: &[DefQuery],
        deadline: Instant,
    ) -> Result<Vec<Option<DefAnswer>>>;
}

/// Auto-detection: every provider whose server + project config are present.
/// Empty on almost all repos — that is the point.
pub fn detect_providers(root: &Path) -> Vec<Box<dyn LspProvider>> {
    let mut out: Vec<Box<dyn LspProvider>> = Vec::new();
    if let Some(ts) = TsServer::detect(root) {
        out.push(Box::new(ts));
    }
    if let Some(py) = Pyright::detect(root) {
        out.push(Box::new(py));
    }
    out
}

/// Cheap probe used to decide whether spawning an enrichment thread is worth
/// it at all.
pub fn available(root: &Path) -> bool {
    TsServer::detect(root).is_some() || Pyright::detect(root).is_some()
}

// ---------------------------------------------------------------------------
// tsserver (TypeScript pilot)
// ---------------------------------------------------------------------------

/// TypeScript's own `tsserver`, spoken over its native protocol (NOT LSP):
/// requests are single-line JSON on stdin; responses/events come back framed
/// as `Content-Length: N\r\n\r\n<json>\n` where N includes the trailing
/// newline.
pub struct TsServer {
    node: PathBuf,
    server_js: PathBuf,
}

impl TsServer {
    /// Requires ALL of: `tsconfig.json` (or `jsconfig.json`) at root, a
    /// project-local `node_modules/typescript/lib/tsserver.js`, and a `node`
    /// binary on PATH. Any missing piece = no enrichment, silently.
    pub fn detect(root: &Path) -> Option<TsServer> {
        let has_config =
            root.join("tsconfig.json").is_file() || root.join("jsconfig.json").is_file();
        if !has_config {
            return None;
        }
        let server_js = root.join("node_modules/typescript/lib/tsserver.js");
        if !server_js.is_file() {
            return None;
        }
        Some(TsServer {
            node: find_node()?,
            server_js,
        })
    }

    /// Test/measurement entry: build against an explicit tsserver.js without
    /// the detection gates.
    pub fn with_server_path(server_js: PathBuf) -> Option<TsServer> {
        Some(TsServer {
            node: find_node()?,
            server_js,
        })
    }
}

fn find_node() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["node.exe", "node.cmd", "node"]
    } else {
        &["node"]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

impl LspProvider for TsServer {
    fn name(&self) -> &'static str {
        "tsserver"
    }

    fn handles(&self, lang: Lang) -> bool {
        matches!(lang, Lang::TypeScript | Lang::Tsx | Lang::JavaScript)
    }

    fn resolve(
        &mut self,
        root: &Path,
        queries: &[DefQuery],
        deadline: Instant,
    ) -> Result<Vec<Option<DefAnswer>>> {
        let mut session = TsSession::spawn(&self.node, &self.server_js, root)?;
        let mut answers: Vec<Option<DefAnswer>> = vec![None; queries.len()];
        let mut opened: rustc_hash::FxHashSet<&str> = Default::default();
        for (i, q) in queries.iter().enumerate() {
            if Instant::now() >= deadline {
                break; // abandon the rest; partial answers still count
            }
            let abs = root.join(&q.file);
            let abs_str = wire_path(&abs);
            if opened.insert(q.file.as_str()) {
                // `open` has no response; project load happens lazily and the
                // first definition request below absorbs the wait.
                session.request("open", serde_json::json!({ "file": abs_str }))?;
            }
            let seq = session.request(
                "definition",
                serde_json::json!({ "file": abs_str, "line": q.line, "offset": q.col }),
            )?;
            let Some(resp) = session.wait_response(seq, deadline) else {
                // Server stopped answering — treat the whole rest as gone.
                break;
            };
            answers[i] = definition_from_response(&resp, root);
        }
        session.shutdown();
        Ok(answers)
    }
}

/// Path as sent to tsserver/node: Windows verbatim prefixes (`\\?\C:\...`,
/// from `canonicalize`) confuse node's path handling and tsserver's own
/// normalization — strip them; keep native separators otherwise.
fn wire_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

/// Maps an absolute path a server answered with back to a root-relative
/// forward-slash path. Canonicalizes to survive /var vs /private/var style
/// symlinked roots (macOS temp dirs). The server may answer in long form
/// while the root canonicalized to a Windows verbatim path (or vice versa) —
/// and the answer file need not exist locally, so canonicalizing the answer
/// is a fallback, not the primary. Tries the root in both spellings first.
/// `None` = the definition lives outside the indexed tree.
fn root_relative(abs: &Path, root: &Path) -> Option<String> {
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let plain_root = PathBuf::from(wire_path(&canon_root));
    let rel = abs
        .strip_prefix(&canon_root)
        .or_else(|_| abs.strip_prefix(&plain_root))
        .ok()
        .map(|p| p.to_path_buf())
        .or_else(|| {
            let c = abs.canonicalize().ok()?;
            c.strip_prefix(&canon_root)
                .or_else(|_| c.strip_prefix(&plain_root))
                .ok()
                .map(|p| p.to_path_buf())
        })?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// First span of a successful `definition` response, mapped root-relative.
fn definition_from_response(resp: &serde_json::Value, root: &Path) -> Option<DefAnswer> {
    if resp.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let span = resp.get("body")?.as_array()?.first()?;
    let file = span.get("file")?.as_str()?;
    let line = span.get("start")?.get("line")?.as_u64()? as u32;
    let rel = root_relative(&PathBuf::from(file), root)?;
    Some(DefAnswer { file: rel, line })
}

/// A live tsserver child. Requests go down stdin as newline-delimited JSON; a
/// reader thread decodes the framed responses/events into a channel. The
/// child is killed on drop, always.
struct TsSession {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<serde_json::Value>,
    next_seq: u64,
}

impl TsSession {
    fn spawn(node: &Path, server_js: &Path, cwd: &Path) -> Result<TsSession> {
        // node cannot load \\?\-verbatim script paths (module loader rejects
        // them), and a verbatim cwd leaks into the fake server's answer
        // computation — de-verbatim everything that reaches the child.
        let mut child = Command::new(node)
            .arg(wire_path(server_js))
            .arg("--disableAutomaticTypingAcquisition")
            .current_dir(wire_path(cwd))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {} {}", node.display(), server_js.display()))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || read_framed_messages(stdout, tx));
        Ok(TsSession {
            child,
            stdin,
            rx,
            next_seq: 1,
        })
    }

    /// Sends one request (single-line JSON + `\n`) and returns its seq.
    fn request(&mut self, command: &str, arguments: serde_json::Value) -> Result<u64> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let req = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        Ok(seq)
    }

    /// Pulls messages until the response matching `seq` arrives (events and
    /// unrelated responses are skipped) or the deadline passes.
    fn wait_response(&mut self, seq: u64, deadline: Instant) -> Option<serde_json::Value> {
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let msg = self.rx.recv_timeout(left).ok()?;
            if msg.get("type").and_then(|v| v.as_str()) == Some("response")
                && msg.get("request_seq").and_then(|v| v.as_u64()) == Some(seq)
            {
                return Some(msg);
            }
        }
    }

    fn shutdown(&mut self) {
        // Polite exit first (tsserver honors it and cleans up its watchers),
        // then the Drop kill as the backstop.
        let _ = self.request("exit", serde_json::json!({}));
        let waited = Instant::now();
        while waited.elapsed() < Duration::from_millis(500) {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
    }
}

impl Drop for TsSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Decodes tsserver's framed output: `Content-Length: N` header line(s), a
/// blank line, then exactly N bytes of JSON (N includes tsserver's trailing
/// newline). Anything unparseable ends the stream.
fn read_framed_messages(stdout: impl Read, tx: mpsc::Sender<serde_json::Value>) {
    let mut r = BufReader::new(stdout);
    loop {
        let mut content_len: Option<usize> = None;
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => return, // EOF / broken pipe
                Ok(_) => {}
            }
            let t = line.trim();
            if t.is_empty() {
                if content_len.is_some() {
                    break;
                }
                continue; // stray blank line before any header
            }
            if let Some(v) = t.strip_prefix("Content-Length:") {
                content_len = v.trim().parse().ok();
            }
            // Other header lines (none today) are skipped.
        }
        let n = content_len.unwrap_or(0);
        let mut buf = vec![0u8; n];
        if r.read_exact(&mut buf).is_err() {
            return;
        }
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf) {
            if tx.send(v).is_err() {
                return; // consumer gone
            }
        }
    }
}

// ---------------------------------------------------------------------------
// pyright (Python, real LSP)
// ---------------------------------------------------------------------------

/// Pyright spoken over actual LSP: JSON-RPC 2.0 with Content-Length framing
/// in BOTH directions (tsserver frames only its output). Lifecycle:
/// initialize -> initialized -> didOpen per file -> textDocument/definition
/// per site -> shutdown/exit. Positions on the wire are 0-based (ours are
/// 1-based) and `character` is what the extractor recorded as a byte column —
/// correct for ASCII, same accepted caveat as tsserver's `offset`.
pub struct Pyright {
    launch: PyrightLaunch,
}

enum PyrightLaunch {
    /// `node node_modules/pyright/langserver.index.js --stdio` (npm install).
    Node { node: PathBuf, script: PathBuf },
    /// A `pyright-langserver` executable (pip install ships one): local venv
    /// or PATH.
    Binary(PathBuf),
}

impl Pyright {
    /// Any of, most project-local first: `node_modules/pyright/
    /// langserver.index.js` + node on PATH (npm install), `pyright-langserver`
    /// inside a local venv (`.venv`/`venv`), or `pyright-langserver` on PATH.
    /// Nothing found = no enrichment, silently. Node is needed only for the
    /// npm variant.
    pub fn detect(root: &Path) -> Option<Pyright> {
        let script = root.join("node_modules/pyright/langserver.index.js");
        if script.is_file() {
            if let Some(node) = find_node() {
                return Some(Pyright {
                    launch: PyrightLaunch::Node { node, script },
                });
            }
        }
        let names: &[&str] = if cfg!(windows) {
            &[
                "pyright-langserver.exe",
                "pyright-langserver.cmd",
                "pyright-langserver",
            ]
        } else {
            &["pyright-langserver"]
        };
        let venv_bin = if cfg!(windows) { "Scripts" } else { "bin" };
        for venv in [".venv", "venv"] {
            for name in names {
                let cand = root.join(venv).join(venv_bin).join(name);
                if cand.is_file() {
                    return Some(Pyright {
                        launch: PyrightLaunch::Binary(cand),
                    });
                }
            }
        }
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            for name in names {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(Pyright {
                        launch: PyrightLaunch::Binary(cand),
                    });
                }
            }
        }
        None
    }
}

impl LspProvider for Pyright {
    fn name(&self) -> &'static str {
        "pyright"
    }

    fn handles(&self, lang: Lang) -> bool {
        matches!(lang, Lang::Python)
    }

    fn resolve(
        &mut self,
        root: &Path,
        queries: &[DefQuery],
        deadline: Instant,
    ) -> Result<Vec<Option<DefAnswer>>> {
        let mut session = LspSession::spawn(&self.launch, root)?;
        // Project analysis makes the initialize round-trip the slow part;
        // it shares the batch deadline like every query after it.
        session.initialize(root, deadline)?;
        let mut answers: Vec<Option<DefAnswer>> = vec![None; queries.len()];
        let mut opened: rustc_hash::FxHashSet<&str> = Default::default();
        for (i, q) in queries.iter().enumerate() {
            if Instant::now() >= deadline {
                break; // abandon the rest; partial answers still count
            }
            let abs = root.join(&q.file);
            let uri = path_to_uri(&abs);
            if opened.insert(q.file.as_str()) {
                let Ok(text) = std::fs::read_to_string(&abs) else {
                    continue; // vanished since indexing; queries there fail soft
                };
                session.notify(
                    "textDocument/didOpen",
                    serde_json::json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": "python",
                            "version": 1,
                            "text": text,
                        }
                    }),
                )?;
            }
            let id = session.request(
                "textDocument/definition",
                serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": {
                        "line": q.line.saturating_sub(1),
                        "character": q.col.saturating_sub(1),
                    }
                }),
            )?;
            let Some(resp) = session.wait_response(id, deadline) else {
                // Server stopped answering — treat the whole rest as gone.
                break;
            };
            answers[i] = resp
                .get("result")
                .and_then(|r| definition_from_lsp_result(r, root));
        }
        session.shutdown(deadline);
        Ok(answers)
    }
}

/// First location of a `textDocument/definition` result, mapped
/// root-relative. LSP allows three shapes: `Location`, `Location[]`, and
/// `LocationLink[]` (pyright sends links when the client advertises support;
/// plain locations otherwise — handle all three regardless). Lines come back
/// 0-based. Definitions outside the indexed tree (typeshed, site-packages)
/// map to `None`.
fn definition_from_lsp_result(result: &serde_json::Value, root: &Path) -> Option<DefAnswer> {
    let first = match result {
        serde_json::Value::Array(items) => items.first()?,
        obj @ serde_json::Value::Object(_) => obj,
        _ => return None,
    };
    let (uri, range) = if let Some(uri) = first.get("uri") {
        (uri.as_str()?, first.get("range")?)
    } else {
        // LocationLink: targetSelectionRange points at the name token itself
        // (best match for `function_at`'s exact-start-line rule).
        (
            first.get("targetUri")?.as_str()?,
            first
                .get("targetSelectionRange")
                .or_else(|| first.get("targetRange"))?,
        )
    };
    let line0 = range.get("start")?.get("line")?.as_u64()? as u32;
    let path = uri_to_path(uri)?;
    let rel = root_relative(&path, root)?;
    Some(DefAnswer {
        file: rel,
        line: line0 + 1,
    })
}

/// `file://` URI for an absolute path: verbatim prefix stripped (node and
/// pyright both mishandle `\\?\`), backslashes normalized, everything outside
/// the unreserved set percent-encoded. Windows drive paths become
/// `file:///C:/...`.
fn path_to_uri(p: &Path) -> String {
    let s = wire_path(p).replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/'); // drive-letter form
    }
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/'
            | b':' => out.push(b as char),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// `file://` URI back to a path: authority (empty or `localhost`) dropped,
/// percent-escapes decoded, Windows drive-letter form `/C:/...` de-slashed.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path_part = if rest.starts_with('/') {
        rest
    } else {
        &rest[rest.find('/')?..]
    };
    let decoded = percent_decode(path_part);
    let b = decoded.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        Some(PathBuf::from(&decoded[1..]))
    } else {
        Some(PathBuf::from(decoded))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A live LSP child (pyright). Messages go BOTH directions Content-Length
/// framed; the tsserver reader thread is reused unchanged since the inbound
/// framing is identical. The child is killed on drop, always.
struct LspSession {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<serde_json::Value>,
    next_id: u64,
}

impl LspSession {
    fn spawn(launch: &PyrightLaunch, cwd: &Path) -> Result<LspSession> {
        // De-verbatim everything that reaches the child, exactly like
        // TsSession: node rejects \\?\ script paths and a verbatim cwd leaks
        // into path answers.
        let mut cmd = match launch {
            PyrightLaunch::Node { node, script } => {
                let mut c = Command::new(node);
                c.arg(wire_path(script));
                c
            }
            PyrightLaunch::Binary(bin) => Command::new(bin),
        };
        let mut child = cmd
            .arg("--stdio")
            .current_dir(wire_path(cwd))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("spawn pyright langserver")?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || read_framed_messages(stdout, tx));
        Ok(LspSession {
            child,
            stdin,
            rx,
            next_id: 1,
        })
    }

    fn send(&mut self, msg: &serde_json::Value) -> Result<()> {
        let body = serde_json::to_vec(msg)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn initialize(&mut self, root: &Path, deadline: Instant) -> Result<()> {
        let id = self.request(
            "initialize",
            serde_json::json!({
                "processId": std::process::id(),
                "rootUri": path_to_uri(root),
                "capabilities": {},
                "workspaceFolders": null,
            }),
        )?;
        self.wait_response(id, deadline)
            .ok_or_else(|| anyhow!("no initialize response before deadline"))?;
        self.notify("initialized", serde_json::json!({}))?;
        Ok(())
    }

    /// Pulls messages until the response matching `id` arrives or the
    /// deadline passes. Notifications (startup progress, diagnostics, logs —
    /// pyright is chatty while it analyzes the project) are skipped;
    /// server->client REQUESTS get a minimal reply so the server never stalls
    /// waiting on us (pyright asks for workspace/configuration).
    fn wait_response(&mut self, id: u64, deadline: Instant) -> Option<serde_json::Value> {
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let msg = self.rx.recv_timeout(left).ok()?;
            match (msg.get("id"), msg.get("method").and_then(|m| m.as_str())) {
                (Some(req_id), Some(method)) => {
                    // Server->client request. Answer with the emptiest thing
                    // that satisfies the shape; ids echo back verbatim (they
                    // may be strings).
                    let result = if method == "workspace/configuration" {
                        let n = msg
                            .pointer("/params/items")
                            .and_then(|v| v.as_array())
                            .map_or(0, |a| a.len());
                        serde_json::Value::Array(vec![serde_json::Value::Null; n])
                    } else {
                        serde_json::Value::Null
                    };
                    let reply = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": result,
                    });
                    let _ = self.send(&reply);
                }
                (Some(resp_id), None) => {
                    if resp_id.as_u64() == Some(id) {
                        return Some(msg);
                    }
                    // Stale response to an abandoned request: skip.
                }
                _ => {} // notification
            }
        }
    }

    fn shutdown(&mut self, deadline: Instant) {
        // Polite LSP exit first (shutdown request, then exit notification),
        // the Drop kill as the backstop.
        if let Ok(id) = self.request("shutdown", serde_json::Value::Null) {
            let grace = Instant::now() + Duration::from_millis(500);
            let _ = self.wait_response(id, deadline.min(grace));
        }
        let _ = self.notify("exit", serde_json::Value::Null);
        let waited = Instant::now();
        while waited.elapsed() < Duration::from_millis(500) {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Enrichment pass
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub struct EnrichOutcome {
    /// Server agreed with the static pick (edge upgraded to High).
    pub confirmed: u32,
    /// Server named a different indexed function (edge re-pointed, High).
    pub corrected: u32,
    /// Server placed the definition in node_modules: the static internal
    /// edge was a false positive, rewritten to External (counted within
    /// `corrected` for the log line, tracked separately here).
    pub externalized: u32,
    /// Candidate sites the server could not settle (left untouched).
    pub unresolved: u32,
    /// Definition requests actually sent (cache hits excluded).
    pub queried: u32,
}

/// Per-file cache of language-server answers keyed by the caller file's
/// content hash — re-enrichment after edits only re-queries changed files.
/// Values are (name_line, name_col) -> definition; `None` remembers "server
/// had no answer" so unresolvable sites aren't re-asked forever.
#[derive(Serialize, Deserialize, Default)]
struct LspCache {
    files: FxHashMap<String, FileAnswers>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct FileAnswers {
    hash: u64,
    answers: FxHashMap<(u32, u32), Option<(String, u32)>>,
}

fn lsp_cache_path(root: &Path) -> PathBuf {
    root.join(indexer::CACHE_DIR).join(LSP_CACHE_FILE)
}

fn load_lsp_cache(root: &Path) -> LspCache {
    std::fs::read(lsp_cache_path(root))
        .ok()
        .and_then(|b| bincode::deserialize(&b).ok())
        .unwrap_or_default()
}

/// Call indices worth asking a language server about: resolved internally but
/// Heuristic or carrying ambiguity, in a language the provider handles, with
/// a known name position. Ambiguous sites first (they gain the most), then
/// call order; capped at `MAX_SITES`.
fn candidate_sites(g: &GigaGraph, provider: &dyn LspProvider) -> Vec<u32> {
    let mut ambiguous: Vec<u32> = Vec::new();
    let mut heuristic: Vec<u32> = Vec::new();
    for (idx, call) in g.calls.iter().enumerate() {
        let Resolution::Internal {
            confidence,
            ambiguous_with,
            ..
        } = &call.resolution
        else {
            continue;
        };
        if *confidence == Confidence::High && ambiguous_with.is_empty() {
            continue;
        }
        if call.name_line == 0 || call.name_col == 0 {
            continue; // stale extraction cache without name positions
        }
        let file = g.file_of(call.caller);
        if !provider.handles(file.language) {
            continue;
        }
        if ambiguous_with.is_empty() {
            heuristic.push(idx as u32);
        } else {
            ambiguous.push(idx as u32);
        }
    }
    ambiguous.extend(heuristic);
    ambiguous.truncate(MAX_SITES);
    ambiguous
}

/// Maps a definition answer to an indexed function: exact `start_line` match
/// first (tsserver points at the name token, which sits on the definition
/// line), else the innermost (smallest) span containing the line. Synthetic
/// toplevels never match.
fn function_at(g: &GigaGraph, ans: &DefAnswer) -> Option<u32> {
    let &fid = g.path_index.get(&ans.file)?;
    let mut containing: Option<u32> = None;
    let mut containing_span = u32::MAX;
    for f in g.functions.iter().filter(|f| f.file_id == fid && !f.is_toplevel) {
        if f.start_line == ans.line {
            return Some(f.id);
        }
        if f.start_line <= ans.line && ans.line <= f.end_line {
            let span = f.end_line - f.start_line;
            if span < containing_span {
                containing_span = span;
                containing = Some(f.id);
            }
        }
    }
    containing
}

/// Rewrites one call site to the server-confirmed callee and keeps the
/// `callers_of` reverse index consistent. Returns true when the callee
/// actually changed (a correction, not just a confirmation).
fn apply_answer(g: &mut GigaGraph, call_idx: u32, confirmed: u32) -> bool {
    let call = &mut g.calls[call_idx as usize];
    let Resolution::Internal {
        callee,
        confidence,
        ambiguous_with,
    } = &mut call.resolution
    else {
        return false;
    };
    let old = *callee;
    let changed = old != confirmed;
    *callee = confirmed;
    *confidence = Confidence::High;
    ambiguous_with.clear();
    if changed {
        if let Some(v) = g.callers_of.get_mut(&old) {
            v.retain(|&i| i != call_idx);
            if v.is_empty() {
                g.callers_of.remove(&old);
            }
        }
        g.callers_of.entry(confirmed).or_default().push(call_idx);
    }
    g.lsp_confirmed.insert(call_idx);
    changed
}

/// The definition lives under node_modules: the static internal pick was a
/// false positive (e.g. joi's `.validate()` credited to an in-repo
/// `validate`). Rewrite the edge to External and keep the reverse indexes
/// consistent.
fn externalize(g: &mut GigaGraph, call_idx: u32, package: &str) -> bool {
    let call = &mut g.calls[call_idx as usize];
    let Resolution::Internal { callee, .. } = &call.resolution else {
        return false;
    };
    let old = *callee;
    call.resolution = Resolution::External {
        package: package.to_string(),
    };
    if let Some(v) = g.callers_of.get_mut(&old) {
        v.retain(|&i| i != call_idx);
        if v.is_empty() {
            g.callers_of.remove(&old);
        }
    }
    g.package_calls
        .entry(package.to_string())
        .or_default()
        .push(call_idx);
    g.lsp_confirmed.insert(call_idx);
    true
}

/// Package attribution for a root-relative definition path inside
/// node_modules: `node_modules/joi/lib/index.d.ts` -> `joi`;
/// `node_modules/@scope/pkg/...` -> `@scope/pkg`; TypeScript's own bundled
/// lib files (`lib.es5.d.ts` et al) -> `builtin:js`. Innermost
/// node_modules wins for nested installs.
fn external_package_for(rel_path: &str) -> Option<String> {
    let idx = rel_path.rfind("node_modules/")?;
    let rest = &rel_path[idx + "node_modules/".len()..];
    if rest.starts_with("typescript/lib/lib.") {
        return Some("builtin:js".to_string());
    }
    let mut segs = rest.split('/');
    let first = segs.next()?;
    if first.is_empty() {
        return None;
    }
    if let Some(stripped) = first.strip_prefix('@') {
        let second = segs.next()?;
        if stripped.is_empty() || second.is_empty() {
            return None;
        }
        Some(format!("{first}/{second}"))
    } else {
        Some(first.to_string())
    }
}

/// The enrichment pass over an already-built index. Mutates the graph in
/// place, stamps the stats, persists index.bin, and updates the per-file
/// answer cache. Never fails the index: provider trouble = partial or zero
/// enrichment, silently.
pub fn enrich_index(
    index: &mut Index,
    root: &Path,
    providers: &mut [Box<dyn LspProvider>],
) -> EnrichOutcome {
    let deadline = Instant::now() + BATCH_BUDGET;
    let mut cache = load_lsp_cache(root);
    let mut outcome = EnrichOutcome::default();

    for provider in providers.iter_mut() {
        let sites = candidate_sites(&index.graph, provider.as_ref());
        if sites.is_empty() {
            continue;
        }
        eprintln!(
            "gigagraph: lsp enrichment ({}): {} candidate sites",
            provider.name(),
            sites.len()
        );

        // Split cached vs live. Cached answers (hash-matched) replay without
        // touching the server.
        let mut resolved: Vec<(u32, Option<DefAnswer>)> = Vec::new();
        let mut live_sites: Vec<u32> = Vec::new();
        let mut live_queries: Vec<DefQuery> = Vec::new();
        for &idx in &sites {
            let call = &index.graph.calls[idx as usize];
            let file = index.graph.file_of(call.caller);
            let cached = cache.files.get(&file.path).and_then(|fa| {
                if fa.hash != file.content_hash {
                    return None;
                }
                fa.answers.get(&(call.name_line, call.name_col)).cloned()
            });
            match cached {
                Some(ans) => resolved.push((
                    idx,
                    ans.map(|(f, l)| DefAnswer { file: f, line: l }),
                )),
                None => {
                    live_sites.push(idx);
                    live_queries.push(DefQuery {
                        file: file.path.clone(),
                        line: call.name_line,
                        col: call.name_col,
                    });
                }
            }
        }

        if !live_queries.is_empty() {
            match provider.resolve(root, &live_queries, deadline) {
                Ok(answers) => {
                    outcome.queried += live_queries.len() as u32;
                    for ((&idx, q), ans) in
                        live_sites.iter().zip(&live_queries).zip(answers)
                    {
                        // Only cache slots the server actually got to; an
                        // abandoned tail stays un-cached and un-counted.
                        if Instant::now() >= deadline && ans.is_none() {
                            // Can't distinguish "no answer" from "never
                            // asked" post-deadline; skip caching the tail.
                            continue;
                        }
                        let call = &index.graph.calls[idx as usize];
                        let file = index.graph.file_of(call.caller);
                        let entry =
                            cache.files.entry(file.path.clone()).or_default();
                        if entry.hash != file.content_hash {
                            entry.hash = file.content_hash;
                            entry.answers.clear();
                        }
                        entry.answers.insert(
                            (q.line, q.col),
                            ans.as_ref().map(|a| (a.file.clone(), a.line)),
                        );
                        resolved.push((idx, ans));
                    }
                }
                Err(e) => {
                    eprintln!(
                        "gigagraph: lsp enrichment ({}) failed: {e:#}",
                        provider.name()
                    );
                }
            }
        }

        for (idx, ans) in resolved {
            let Some(a) = ans else {
                outcome.unresolved += 1;
                continue;
            };
            if let Some(fn_id) = function_at(&index.graph, &a) {
                if apply_answer(&mut index.graph, idx, fn_id) {
                    outcome.corrected += 1;
                } else {
                    outcome.confirmed += 1;
                }
            } else if let Some(pkg) = external_package_for(&a.file) {
                if externalize(&mut index.graph, idx, &pkg) {
                    outcome.corrected += 1;
                    outcome.externalized += 1;
                } else {
                    outcome.unresolved += 1;
                }
            } else {
                outcome.unresolved += 1;
            }
        }
        eprintln!(
            "gigagraph: lsp enrichment ({}): {} sites confirmed, {} corrected, {} unresolved",
            provider.name(),
            outcome.confirmed,
            outcome.corrected,
            outcome.unresolved
        );
    }

    index.stats.lsp_enriched = true;
    index.stats.lsp_confirmed_calls = outcome.confirmed;
    index.stats.lsp_corrected_calls = outcome.corrected;
    // Externalized edges moved between resolution buckets.
    if outcome.externalized > 0 {
        index.stats.resolved_internal =
            index.stats.resolved_internal.saturating_sub(outcome.externalized);
        index.stats.resolved_external += outcome.externalized;
        index.stats.external_packages = index.graph.package_calls.len() as u32;
    }
    if let Ok(bytes) = bincode::serialize(&cache) {
        let _ = std::fs::create_dir_all(root.join(indexer::CACHE_DIR));
        let _ = indexer::atomic_write(&lsp_cache_path(root), &bytes);
    }
    if let Err(e) = indexer::persist_index(root, index) {
        eprintln!("gigagraph: warning: failed to persist enriched index: {e}");
    }
    outcome
}

/// Background-thread entry: reload the just-persisted index from disk (the
/// serve loop keeps its own copy untouched), enrich, persist, and hand the
/// enriched index back for adoption. `None` when the tree moved on, the work
/// was already done, or no provider is available.
pub fn enrich_root(root: &Path, expected_fingerprint: u64) -> Option<Index> {
    let mut index = indexer::load_index(root)?;
    if index.stats.tree_fingerprint != expected_fingerprint || index.stats.lsp_enriched {
        return None;
    }
    let mut providers = detect_providers(root);
    if providers.is_empty() {
        return None;
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    enrich_index(&mut index, &root, &mut providers);
    Some(index)
}
