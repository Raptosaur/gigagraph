//! LSP hybrid-resolution enrichment (TypeScript pilot).
//!
//! Protocol framing and the enrichment pass are exercised against a scripted
//! fake tsserver (tests/fixtures/lsp/fake_tsserver.js) that speaks the real
//! wire shape: newline-delimited JSON requests in, Content-Length-framed
//! responses/events out. Tests are gated on a `node` binary being present —
//! absent node, they pass vacuously (exactly like the enrichment itself).

use gigagraph::indexer::{Index, build_index};
use gigagraph::lsp::{self, DefQuery, LspProvider, Pyright, TsServer};
use gigagraph::types::{Confidence, Lang, Resolution};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn fake_tsserver_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lsp/fake_tsserver.js")
}

/// Writes a tiny TS project whose `svc.process()` call is statically
/// ambiguous (two same-named methods, same directory, untyped receiver) into
/// `dir`, with the fake tsserver installed at the canonical project-local
/// path so real detection (`tsconfig.json` + `node_modules/typescript/lib/
/// tsserver.js` + node on PATH) finds it unmodified.
fn write_fixture(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let lib = dir.join("node_modules/typescript/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::copy(fake_tsserver_src(), lib.join("tsserver.js")).unwrap();
    std::fs::write(
        dir.join("tsconfig.json"),
        "{\n  \"compilerOptions\": { \"strict\": true }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("a.ts"),
        "export class Alpha {\n  process(): number {\n    return 1;\n  }\n\n  validate(): boolean {\n    return true;\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.ts"),
        "export class Beta {\n  process(): number {\n    return 2;\n  }\n\n  validate(): boolean {\n    return false;\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("factory.ts"),
        "import { Beta } from \"./b\";\n\nexport function getService(): Beta {\n  return new Beta();\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.ts"),
        "import { getService } from \"./factory\";\n\nexport function run(): number {\n  const svc = getService();\n  return svc.process();\n}\n\nexport function ping(): number {\n  const q = getService();\n  if (q.validate()) {\n    return 1;\n  }\n  return 0;\n}\n",
    )
    .unwrap();
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("gigagraph-lsp-{name}-{}", std::process::id()))
}

/// The `svc.process()` call site index in main.ts, plus the two candidate
/// method ids (Alpha.process, Beta.process).
fn find_process_call(index: &Index) -> (u32, u32, u32) {
    let g = &index.graph;
    let call_idx = g
        .calls
        .iter()
        .position(|c| c.name == "process" && c.receiver.as_deref() == Some("svc"))
        .expect("svc.process() call site indexed") as u32;
    let alpha = g.qname_index["a::Alpha::process"][0];
    let beta = g.qname_index["b::Beta::process"][0];
    (call_idx, alpha, beta)
}

fn write_answers(dir: &Path, entries: &[(&str, u32, u32, &str, u32)]) {
    let mut map = serde_json::Map::new();
    for (file, line, col, def_file, def_line) in entries {
        map.insert(
            format!("{file}:{line}:{col}"),
            serde_json::json!({ "file": def_file, "line": def_line }),
        );
    }
    std::fs::write(
        dir.join("node_modules/typescript/lib/answers.json"),
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Protocol framing against the scripted fake server
// ---------------------------------------------------------------------------

#[test]
fn protocol_framing_against_fake_tsserver() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("proto");
    write_fixture(&dir);
    // Canned answers: one in-project definition, one pointing outside the
    // project root (must map to None), one missing key (success:false).
    write_answers(
        &dir,
        &[
            ("main.ts", 5, 14, "b.ts", 2),
            ("main.ts", 4, 15, "/definitely/outside/x.ts", 1),
        ],
    );

    let mut ts = TsServer::detect(&dir).expect("detection finds config + local tsserver + node");
    assert_eq!(ts.name(), "tsserver");
    let deadline = Instant::now() + Duration::from_secs(10);
    let queries = vec![
        DefQuery { file: "main.ts".into(), line: 5, col: 14 },
        DefQuery { file: "main.ts".into(), line: 4, col: 15 },
        DefQuery { file: "main.ts".into(), line: 1, col: 1 },
    ];
    let answers = ts.resolve(&dir, &queries, deadline).expect("resolve succeeds");
    assert_eq!(answers.len(), 3);
    // In-project definition maps root-relative; interleaved events (emitted
    // by the fake on open) were skipped correctly.
    let a0 = answers[0].as_ref().expect("in-project definition answered");
    assert_eq!(a0.file, "b.ts");
    assert_eq!(a0.line, 2);
    // Definition outside the indexed tree yields no usable answer.
    assert!(answers[1].is_none(), "outside-root definition must map to None");
    // Position with no canned answer -> success:false -> None.
    assert!(answers[2].is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// End-to-end enrichment on the ambiguous fixture
// ---------------------------------------------------------------------------

#[test]
fn enrichment_corrects_ambiguous_ts_call() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("enrich");
    write_fixture(&dir);

    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, alpha, beta) = find_process_call(&index);

    // Static baseline: honest Heuristic with the other candidate listed.
    let call = &index.graph.calls[call_idx as usize];
    let (static_callee, static_conf, static_amb) = match &call.resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            (*callee, *confidence, ambiguous_with.clone())
        }
        other => panic!("expected Internal resolution, got {other:?}"),
    };
    assert_eq!(static_conf, Confidence::Heuristic);
    assert!(!static_amb.is_empty(), "static pick should carry ambiguity");
    assert!(call.name_line > 0 && call.name_col > 0, "name position captured");

    // Second candidate: `q.validate()` — statically ambiguous between
    // Alpha.validate and Beta.validate, but the server places the definition
    // in node_modules (the joi-misattribution shape seen in the wild).
    let validate_idx = index
        .graph
        .calls
        .iter()
        .position(|c| c.name == "validate" && c.receiver.as_deref() == Some("q"))
        .expect("q.validate() call site indexed") as u32;
    let vcall = &index.graph.calls[validate_idx as usize];
    assert!(matches!(
        &vcall.resolution,
        Resolution::Internal { confidence: Confidence::Heuristic, .. }
    ));
    let (v_line, v_col) = (vcall.name_line, vcall.name_col);

    // The language server names Beta.process — write the canned answers at
    // the exact positions the extractor recorded for the call names.
    let beta_line = index.graph.functions[beta as usize].start_line;
    write_answers(
        &dir,
        &[
            ("main.ts", call.name_line, call.name_col, "b.ts", beta_line),
            ("main.ts", v_line, v_col, "node_modules/joifake/lib/index.d.ts", 3),
        ],
    );

    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    assert_eq!(providers.len(), 1, "tsserver should be auto-detected");
    let outcome = lsp::enrich_index(&mut index, &root, &mut providers);

    // The edge is settled: callee = Beta.process, High, no ambiguity, and
    // marked as LSP-confirmed.
    match &index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, beta, "server answer wins");
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal resolution, got {other:?}"),
    }
    assert!(index.graph.lsp_confirmed.contains(&call_idx));
    if static_callee == beta {
        assert_eq!(outcome.confirmed, 1);
    } else {
        assert_eq!(static_callee, alpha, "static pick was one of the two");
        assert_eq!(outcome.corrected - outcome.externalized, 1);
        // Reverse index followed the rewrite.
        assert!(
            !index.graph.callers_of.get(&alpha).is_some_and(|v| v.contains(&call_idx)),
            "old callee must not list the corrected call"
        );
    }
    assert!(
        index.graph.callers_of.get(&beta).is_some_and(|v| v.contains(&call_idx)),
        "confirmed callee gains the caller edge"
    );

    // The node_modules answer rewrote the false internal edge to External.
    assert_eq!(outcome.externalized, 1);
    match &index.graph.calls[validate_idx as usize].resolution {
        Resolution::External { package } => assert_eq!(package, "joifake"),
        other => panic!("expected External resolution, got {other:?}"),
    }
    assert!(index.graph.lsp_confirmed.contains(&validate_idx));
    assert!(
        index
            .graph
            .package_calls
            .get("joifake")
            .is_some_and(|v| v.contains(&validate_idx)),
        "externalized edge joins package_calls"
    );
    assert!(
        !index
            .graph
            .callers_of
            .values()
            .any(|v| v.contains(&validate_idx)),
        "externalized edge left callers_of entirely"
    );

    // Stamped and persisted: a reload sees enriched work, no redo needed.
    assert!(index.stats.lsp_enriched);
    let reloaded = gigagraph::indexer::load_index(&root).expect("persisted index reloads");
    assert!(reloaded.stats.lsp_enriched);
    assert!(reloaded.graph.lsp_confirmed.contains(&call_idx));
    assert!(dir.join(".gigagraph/lsp.bin").is_file(), "answer cache persisted");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cached_answers_replay_without_touching_the_server() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("cache");
    write_fixture(&dir);

    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, _alpha, beta) = find_process_call(&index);
    let call = &index.graph.calls[call_idx as usize];
    let beta_line = index.graph.functions[beta as usize].start_line;
    write_answers(
        &dir,
        &[("main.ts", call.name_line, call.name_col, "b.ts", beta_line)],
    );

    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    let first = lsp::enrich_index(&mut index, &root, &mut providers);
    assert_eq!(first.confirmed + first.corrected, 1);
    assert_eq!(first.unresolved, 1, "the un-canned q.validate() site stays put");
    assert!(first.queried >= 1, "first pass hits the live server");

    // Rebuild from scratch (static resolution is ambiguous again), then
    // enrich with the canned answers REMOVED: only the lsp.bin cache can
    // settle the edge now — and it must, without any live query. The cached
    // "no answer" for q.validate() replays too, instead of being re-asked.
    let mut second_index = build_index(&dir, true).expect("rebuild");
    std::fs::remove_file(dir.join("node_modules/typescript/lib/answers.json")).unwrap();
    let mut providers = lsp::detect_providers(&root);
    let second = lsp::enrich_index(&mut second_index, &root, &mut providers);
    assert_eq!(second.queried, 0, "hash-matched cache answers replay offline");
    assert_eq!(second.confirmed + second.corrected, 1);
    assert_eq!(second.unresolved, 1);
    match &second_index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, .. } => {
            assert_eq!(*callee, beta);
            assert_eq!(*confidence, Confidence::High);
        }
        other => panic!("expected Internal, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// MCP-server path: background enrichment is adopted between tool calls
// ---------------------------------------------------------------------------

#[test]
fn appstate_adopts_background_enrichment_and_surfaces_lsp_confidence() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("appstate");
    write_fixture(&dir);

    // Learn the recorded call-name positions first (fixture content is
    // deterministic), write the canned answers, then start fresh state.
    let index = build_index(&dir, true).expect("index build");
    let (call_idx, _alpha, beta) = find_process_call(&index);
    let call = &index.graph.calls[call_idx as usize];
    let beta_line = index.graph.functions[beta as usize].start_line;
    write_answers(
        &dir,
        &[("main.ts", call.name_line, call.name_col, "b.ts", beta_line)],
    );

    let mut state = gigagraph::api::AppState::new(dir.clone());
    let deadline = Instant::now() + Duration::from_secs(60);
    let confidence_of_process_call = |v: &serde_json::Value| -> Option<String> {
        v["calls"].as_array()?.iter().find_map(|c| {
            (c["name"] == "process").then(|| c["confidence"].as_str().unwrap_or("").to_string())
        })
    };
    // First call sees the plain static answer (heuristic) and kicks off the
    // background pass; later calls adopt the enriched index.
    loop {
        let v = state
            .dispatch("get_callees", &serde_json::json!({ "function": "run" }))
            .expect("get_callees");
        let conf = confidence_of_process_call(&v).expect("svc.process() call listed");
        if conf == "lsp" {
            assert!(
                v["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == "process"
                        && c["callee"] == "b::Beta::process"
                        && c["ambiguous_with"].is_null()),
                "enriched edge points at Beta.process with no ambiguity: {v}"
            );
            break;
        }
        assert_eq!(conf, "heuristic", "pre-enrichment surface stays honest");
        assert!(
            Instant::now() < deadline,
            "background enrichment was never adopted"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The always-works baseline: no config, no server, no enrichment
// ---------------------------------------------------------------------------

#[test]
fn no_op_without_tsconfig_or_local_tsserver() {
    // A TS project without tsconfig/node_modules: nothing detected.
    let dir = temp_root("noop");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.ts"), "export function f(): number { return 1; }\n").unwrap();
    assert!(!lsp::available(&dir));
    assert!(lsp::detect_providers(&dir).is_empty());

    // tsconfig alone is not enough — the project-local server must exist.
    std::fs::write(dir.join("tsconfig.json"), "{}\n").unwrap();
    assert!(!lsp::available(&dir));

    // And the static index is untouched by any of this.
    let index = build_index(&dir, true).expect("index build");
    assert!(!index.stats.lsp_enriched);
    assert!(index.graph.lsp_confirmed.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Real tsserver (not run in CI — needs `npm install typescript` in a fixture)
// ---------------------------------------------------------------------------

/// Run manually with:
///   cd $(mktemp -d) && npm init -y >/dev/null && npm i typescript --no-save
///   GIGAGRAPH_REAL_TS_FIXTURE=$PWD cargo test --test lsp_test real_tsserver -- --ignored
/// after copying the four fixture .ts files + tsconfig.json into that dir
/// (see `write_fixture`) — or point it at any TS repo with node_modules.
#[test]
#[ignore = "requires a real typescript install in the fixture; fake server covers CI"]
fn real_tsserver_disambiguates() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = match std::env::var("GIGAGRAPH_REAL_TS_FIXTURE") {
        Ok(d) => PathBuf::from(d),
        Err(_) => {
            eprintln!("skipping: set GIGAGRAPH_REAL_TS_FIXTURE to a prepared fixture dir");
            return;
        }
    };
    // Overwrite sources with the ambiguous fixture (keeps whatever real
    // node_modules/typescript the caller installed).
    for (name, body) in [
        ("a.ts", "export class Alpha {\n  process(): number {\n    return 1;\n  }\n}\n"),
        ("b.ts", "export class Beta {\n  process(): number {\n    return 2;\n  }\n}\n"),
        (
            "factory.ts",
            "import { Beta } from \"./b\";\n\nexport function getService(): Beta {\n  return new Beta();\n}\n",
        ),
        (
            "main.ts",
            "import { getService } from \"./factory\";\n\nexport function run(): number {\n  const svc = getService();\n  return svc.process();\n}\n",
        ),
        ("tsconfig.json", "{\n  \"compilerOptions\": { \"strict\": true }\n}\n"),
    ] {
        std::fs::write(dir.join(name), body).unwrap();
    }
    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, _alpha, beta) = find_process_call(&index);
    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    assert!(!providers.is_empty(), "real tsserver should be detected");
    let outcome = lsp::enrich_index(&mut index, &root, &mut providers);
    eprintln!("real tsserver outcome: {outcome:?}");
    match &index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, beta, "tsc's type flow names Beta.process");
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal, got {other:?}"),
    }
}

// ===========================================================================
// pyright (Python, real LSP)
// ===========================================================================
//
// Mirrors the tsserver suite against a scripted fake pyright langserver
// (tests/fixtures/lsp/fake_pyright_langserver.js) that speaks actual LSP:
// JSON-RPC 2.0, Content-Length framed in BOTH directions, lifecycle enforced
// (initialize -> initialized -> didOpen before definition), with pyright's
// startup noise (log/progress notifications) and a server->client
// workspace/configuration request the client must answer before any
// definition is released. Node-gated exactly like the tsserver tests.

fn fake_pyright_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lsp/fake_pyright_langserver.js")
}

/// Adds the Python half of the fixture (same shape as the TS one: two
/// same-named methods on different classes, untyped receiver — statically
/// ambiguous, `svc.process()` marked Heuristic with `ambiguous_with`) plus
/// the fake langserver at the npm detection path
/// `node_modules/pyright/langserver.index.js`. Does NOT wipe `dir`, so it
/// can stack on top of the TS fixture for the combined-pass test.
fn add_py_fixture(dir: &Path) {
    let pkg = dir.join("node_modules/pyright");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::copy(fake_pyright_src(), pkg.join("langserver.index.js")).unwrap();
    std::fs::write(
        dir.join("a.py"),
        "class Alpha:\n    def process(self):\n        return 1\n\n    def validate(self):\n        return True\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.py"),
        "class Beta:\n    def process(self):\n        return 2\n\n    def validate(self):\n        return False\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("factory.py"),
        "from b import Beta\n\n\ndef get_service():\n    return Beta()\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.py"),
        "from factory import get_service\n\n\ndef run():\n    svc = get_service()\n    return svc.process()\n\n\ndef ping():\n    q = get_service()\n    if q.validate():\n        return 1\n    return 0\n",
    )
    .unwrap();
}

fn write_py_fixture(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    add_py_fixture(dir);
}

/// answers.json for the fake pyright: keys are 1-based `<basename>:<line>:
/// <col>` of the QUERY (the wire carries 0-based positions; the fake adds 1
/// back, so a client that skipped the conversion misses every key). `shape`
/// picks the LSP result form: "location" | "locations" | "locationLink".
fn write_py_answers(dir: &Path, entries: &[(&str, u32, u32, &str, u32, &str)]) {
    let mut map = serde_json::Map::new();
    for (file, line, col, def_file, def_line, shape) in entries {
        map.insert(
            format!("{file}:{line}:{col}"),
            serde_json::json!({ "file": def_file, "line": def_line, "shape": shape }),
        );
    }
    std::fs::write(
        dir.join("node_modules/pyright/answers.json"),
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap(),
    )
    .unwrap();
}

/// The Python `svc.process()` call site plus the two candidate method ids
/// (Alpha.process in a.py, Beta.process in b.py). Looks the functions up by
/// file path, not qname, so it stays unambiguous when the TS fixture (with
/// its identically named classes) shares the tree.
fn find_py_process_call(index: &Index) -> (u32, u32, u32) {
    let g = &index.graph;
    let call_idx = g
        .calls
        .iter()
        .position(|c| {
            c.name == "process"
                && c.receiver.as_deref() == Some("svc")
                && g.file_of(c.caller).path == "main.py"
        })
        .expect("svc.process() python call site indexed") as u32;
    let fn_in = |path: &str, name: &str| -> u32 {
        let fid = g.path_index[path];
        g.functions
            .iter()
            .find(|f| f.file_id == fid && f.name == name && !f.is_toplevel)
            .unwrap_or_else(|| panic!("{name} in {path} indexed"))
            .id
    };
    (call_idx, fn_in("a.py", "process"), fn_in("b.py", "process"))
}

/// Mirrors detection's PATH scan (the real binary can't be probed with
/// `--version` — it exits nonzero without a connection argument).
fn pyright_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: &[&str] = if cfg!(windows) {
        &["pyright-langserver.exe", "pyright-langserver.cmd", "pyright-langserver"]
    } else {
        &["pyright-langserver"]
    };
    std::env::split_paths(&path).any(|d| names.iter().any(|n| d.join(n).is_file()))
}

// ---------------------------------------------------------------------------
// LSP protocol framing/lifecycle against the scripted fake server
// ---------------------------------------------------------------------------

#[test]
fn pyright_protocol_framing_against_fake_langserver() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("py-proto");
    write_py_fixture(&dir);
    // One answer per LSP result shape, one outside the root, one miss.
    write_py_answers(
        &dir,
        &[
            ("main.py", 6, 16, "b.py", 2, "location"),
            ("main.py", 11, 10, "a.py", 5, "locations"),
            ("a.py", 2, 9, "b.py", 5, "locationLink"),
            ("main.py", 5, 11, "/definitely/outside/x.py", 1, "location"),
        ],
    );

    let mut py = Pyright::detect(&dir).expect("npm-path detection finds the fake langserver");
    assert_eq!(py.name(), "pyright");
    assert!(py.handles(Lang::Python));
    assert!(!py.handles(Lang::TypeScript));
    let deadline = Instant::now() + Duration::from_secs(10);
    let queries = vec![
        DefQuery { file: "main.py".into(), line: 6, col: 16 },
        DefQuery { file: "main.py".into(), line: 11, col: 10 },
        DefQuery { file: "a.py".into(), line: 2, col: 9 },
        DefQuery { file: "main.py".into(), line: 5, col: 11 },
        DefQuery { file: "main.py".into(), line: 1, col: 1 },
    ];
    let answers = py.resolve(&dir, &queries, deadline).expect("resolve succeeds");
    assert_eq!(answers.len(), 5);
    // Plain Location, mapped root-relative, line back to 1-based. The fake's
    // interleaved log/progress notifications and its workspace/configuration
    // request (which gates every definition answer) were all handled.
    assert_eq!(
        answers[0],
        Some(gigagraph::lsp::DefAnswer { file: "b.py".into(), line: 2 })
    );
    // Location[] and LocationLink[] shapes decode the same way.
    assert_eq!(
        answers[1],
        Some(gigagraph::lsp::DefAnswer { file: "a.py".into(), line: 5 })
    );
    assert_eq!(
        answers[2],
        Some(gigagraph::lsp::DefAnswer { file: "b.py".into(), line: 5 })
    );
    // Definition outside the indexed tree yields no usable answer.
    assert!(answers[3].is_none(), "outside-root definition must map to None");
    // Position with no canned answer -> result: null -> None.
    assert!(answers[4].is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// End-to-end enrichment on the ambiguous Python fixture
// ---------------------------------------------------------------------------

#[test]
fn pyright_enrichment_corrects_ambiguous_python_call() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("py-enrich");
    write_py_fixture(&dir);

    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, alpha, beta) = find_py_process_call(&index);

    // Static baseline: honest Heuristic with the other candidate listed
    // (probed: the resolver picks Alpha.process, ambiguous with Beta's).
    let call = &index.graph.calls[call_idx as usize];
    let (static_callee, static_conf, static_amb) = match &call.resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            (*callee, *confidence, ambiguous_with.clone())
        }
        other => panic!("expected Internal resolution, got {other:?}"),
    };
    assert_eq!(static_conf, Confidence::Heuristic);
    assert!(!static_amb.is_empty(), "static pick should carry ambiguity");
    assert!(call.name_line > 0 && call.name_col > 0, "name position captured");

    // Second candidate: `q.validate()` — pyright places the definition in an
    // installed package OUTSIDE the root (site-packages/typeshed shape).
    // Unlike tsserver's node_modules answers (which sit under the root and
    // externalize), out-of-tree Python definitions map to no answer and the
    // static edge honestly keeps its Heuristic label.
    let validate_idx = index
        .graph
        .calls
        .iter()
        .position(|c| c.name == "validate" && c.receiver.as_deref() == Some("q"))
        .expect("q.validate() call site indexed") as u32;
    let vcall = &index.graph.calls[validate_idx as usize];
    assert!(matches!(
        &vcall.resolution,
        Resolution::Internal { confidence: Confidence::Heuristic, .. }
    ));
    let (v_line, v_col) = (vcall.name_line, vcall.name_col);
    let (v_callee, v_amb) = match &vcall.resolution {
        Resolution::Internal { callee, ambiguous_with, .. } => {
            (*callee, ambiguous_with.clone())
        }
        other => panic!("expected Internal, got {other:?}"),
    };

    // The language server names Beta.process — canned at the exact positions
    // the extractor recorded, as a LocationLink (what real pyright sends to
    // link-capable clients).
    let beta_line = index.graph.functions[beta as usize].start_line;
    write_py_answers(
        &dir,
        &[
            ("main.py", call.name_line, call.name_col, "b.py", beta_line, "locationLink"),
            ("main.py", v_line, v_col, "/site-packages/marshmallowfake/schema.py", 3, "location"),
        ],
    );

    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    assert_eq!(providers.len(), 1, "pyright (and only pyright) auto-detected");
    assert_eq!(providers[0].name(), "pyright");
    let outcome = lsp::enrich_index(&mut index, &root, &mut providers);

    // The edge is settled: callee = Beta.process, High, no ambiguity, and
    // marked as LSP-confirmed.
    match &index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, beta, "server answer wins");
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal resolution, got {other:?}"),
    }
    assert!(index.graph.lsp_confirmed.contains(&call_idx));
    if static_callee == beta {
        assert_eq!(outcome.confirmed, 1);
    } else {
        assert_eq!(static_callee, alpha, "static pick was one of the two");
        assert_eq!(outcome.corrected, 1);
        assert!(
            !index.graph.callers_of.get(&alpha).is_some_and(|v| v.contains(&call_idx)),
            "old callee must not list the corrected call"
        );
    }
    assert!(
        index.graph.callers_of.get(&beta).is_some_and(|v| v.contains(&call_idx)),
        "confirmed callee gains the caller edge"
    );

    // The out-of-tree answer settles nothing: edge untouched, still counted
    // unresolved, never externalized (no node_modules attribution).
    assert_eq!(outcome.externalized, 0);
    assert_eq!(outcome.unresolved, 1);
    match &index.graph.calls[validate_idx as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, v_callee, "unsettled edge keeps its static pick");
            assert_eq!(*confidence, Confidence::Heuristic);
            assert_eq!(*ambiguous_with, v_amb);
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    assert!(!index.graph.lsp_confirmed.contains(&validate_idx));

    // Stamped and persisted: a reload sees enriched work, no redo needed.
    assert!(index.stats.lsp_enriched);
    let reloaded = gigagraph::indexer::load_index(&root).expect("persisted index reloads");
    assert!(reloaded.stats.lsp_enriched);
    assert!(reloaded.graph.lsp_confirmed.contains(&call_idx));
    assert!(dir.join(".gigagraph/lsp.bin").is_file(), "answer cache persisted");

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Cache replay (shared lsp.bin machinery, pyright answers)
// ---------------------------------------------------------------------------

#[test]
fn pyright_cached_answers_replay_without_touching_the_server() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("py-cache");
    write_py_fixture(&dir);

    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, _alpha, beta) = find_py_process_call(&index);
    let call = &index.graph.calls[call_idx as usize];
    let beta_line = index.graph.functions[beta as usize].start_line;
    write_py_answers(
        &dir,
        &[("main.py", call.name_line, call.name_col, "b.py", beta_line, "location")],
    );

    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    let first = lsp::enrich_index(&mut index, &root, &mut providers);
    assert_eq!(first.confirmed + first.corrected, 1);
    assert_eq!(first.unresolved, 1, "the un-canned q.validate() site stays put");
    assert!(first.queried >= 1, "first pass hits the live server");

    // Rebuild from scratch, remove the canned answers: only lsp.bin can
    // settle the edge now — and it must, without any live query. The cached
    // "no answer" for q.validate() replays too, instead of being re-asked.
    let mut second_index = build_index(&dir, true).expect("rebuild");
    std::fs::remove_file(dir.join("node_modules/pyright/answers.json")).unwrap();
    let mut providers = lsp::detect_providers(&root);
    let second = lsp::enrich_index(&mut second_index, &root, &mut providers);
    assert_eq!(second.queried, 0, "hash-matched cache answers replay offline");
    assert_eq!(second.confirmed + second.corrected, 1);
    assert_eq!(second.unresolved, 1);
    match &second_index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, .. } => {
            assert_eq!(*callee, beta);
            assert_eq!(*confidence, Confidence::High);
        }
        other => panic!("expected Internal, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Both providers, one enrichment pass, one shared deadline
// ---------------------------------------------------------------------------

#[test]
fn both_providers_enrich_in_one_pass() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let dir = temp_root("both");
    write_fixture(&dir); // TS half (wipes dir first)
    add_py_fixture(&dir); // Python half on top

    let mut index = build_index(&dir, true).expect("index build");
    let g = &index.graph;
    let call_in = |path: &str, name: &str, recv: &str| -> u32 {
        g.calls
            .iter()
            .position(|c| {
                c.name == name
                    && c.receiver.as_deref() == Some(recv)
                    && g.file_of(c.caller).path == path
            })
            .unwrap_or_else(|| panic!("{recv}.{name}() in {path} indexed")) as u32
    };
    let fn_in = |path: &str, name: &str| -> u32 {
        let fid = g.path_index[path];
        g.functions
            .iter()
            .find(|f| f.file_id == fid && f.name == name && !f.is_toplevel)
            .unwrap_or_else(|| panic!("{name} in {path} indexed"))
            .id
    };
    let ts_call = call_in("main.ts", "process", "svc");
    let py_call = call_in("main.py", "process", "svc");
    let ts_beta = fn_in("b.ts", "process");
    let py_beta = fn_in("b.py", "process");
    for idx in [ts_call, py_call] {
        assert!(
            matches!(
                &g.calls[idx as usize].resolution,
                Resolution::Internal { confidence: Confidence::Heuristic, .. }
            ),
            "both fixture halves start statically uncertain"
        );
    }

    let tcall = &g.calls[ts_call as usize];
    let pcall = &g.calls[py_call as usize];
    let ts_beta_line = g.functions[ts_beta as usize].start_line;
    let py_beta_line = g.functions[py_beta as usize].start_line;
    write_answers(
        &dir,
        &[("main.ts", tcall.name_line, tcall.name_col, "b.ts", ts_beta_line)],
    );
    write_py_answers(
        &dir,
        &[("main.py", pcall.name_line, pcall.name_col, "b.py", py_beta_line, "location")],
    );

    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    assert_eq!(
        providers.iter().map(|p| p.name()).collect::<Vec<_>>(),
        vec!["tsserver", "pyright"],
        "both servers detected side by side"
    );
    let outcome = lsp::enrich_index(&mut index, &root, &mut providers);

    // One pass settled a TS edge via tsserver AND a Python edge via pyright.
    assert!(outcome.confirmed + outcome.corrected >= 2);
    match &index.graph.calls[ts_call as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, ts_beta);
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    match &index.graph.calls[py_call as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, py_beta);
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    assert!(index.graph.lsp_confirmed.contains(&ts_call));
    assert!(index.graph.lsp_confirmed.contains(&py_call));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The always-works baseline: no server anywhere, no enrichment
// ---------------------------------------------------------------------------

#[test]
fn pyright_no_op_without_server() {
    if pyright_on_path() {
        // Detection legitimately finds a global pip install; the "nothing
        // detected" assertions below would be wrong on this machine.
        eprintln!("skipping: a real pyright-langserver is on PATH");
        return;
    }
    let dir = temp_root("py-noop");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.py"), "def f():\n    return 1\n").unwrap();
    assert!(!lsp::available(&dir));
    assert!(lsp::detect_providers(&dir).is_empty());

    // An empty node_modules/pyright directory is not enough — the langserver
    // entry point itself must exist.
    std::fs::create_dir_all(dir.join("node_modules/pyright")).unwrap();
    assert!(!lsp::available(&dir));

    // And the static index is untouched by any of this.
    let index = build_index(&dir, true).expect("index build");
    assert!(!index.stats.lsp_enriched);
    assert!(index.graph.lsp_confirmed.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Real pyright (not run in CI — needs a pip/npm pyright install)
// ---------------------------------------------------------------------------

/// Run manually with:
///   d=$(mktemp -d) && python3 -m venv "$d/.venv" && "$d/.venv/bin/pip" install pyright
///   GIGAGRAPH_REAL_PY_FIXTURE=$d cargo test --test lsp_test real_pyright -- --ignored
/// (or `npm i pyright --no-save` in the fixture instead of the venv). The
/// first run downloads pyright's bundled Node runtime for the pip variant, so
/// give it network access.
#[test]
#[ignore = "requires a real pyright install in the fixture; fake server covers CI"]
fn real_pyright_disambiguates() {
    let dir = match std::env::var("GIGAGRAPH_REAL_PY_FIXTURE") {
        Ok(d) => PathBuf::from(d),
        Err(_) => {
            eprintln!("skipping: set GIGAGRAPH_REAL_PY_FIXTURE to a prepared fixture dir");
            return;
        }
    };
    // The ambiguous Python sources, minus the fake server install.
    for (name, body) in [
        (
            "a.py",
            "class Alpha:\n    def process(self):\n        return 1\n",
        ),
        (
            "b.py",
            "class Beta:\n    def process(self):\n        return 2\n",
        ),
        (
            "factory.py",
            "from b import Beta\n\n\ndef get_service():\n    return Beta()\n",
        ),
        (
            "main.py",
            "from factory import get_service\n\n\ndef run():\n    svc = get_service()\n    return svc.process()\n",
        ),
    ] {
        std::fs::write(dir.join(name), body).unwrap();
    }
    let mut index = build_index(&dir, true).expect("index build");
    let (call_idx, _alpha, beta) = find_py_process_call(&index);
    let root = dir.canonicalize().unwrap();
    let mut providers = lsp::detect_providers(&root);
    assert!(
        providers.iter().any(|p| p.name() == "pyright"),
        "real pyright should be detected (venv, npm, or PATH)"
    );
    let outcome = lsp::enrich_index(&mut index, &root, &mut providers);
    eprintln!("real pyright outcome: {outcome:?}");
    match &index.graph.calls[call_idx as usize].resolution {
        Resolution::Internal { callee, confidence, ambiguous_with } => {
            assert_eq!(*callee, beta, "pyright's type flow names Beta.process");
            assert_eq!(*confidence, Confidence::High);
            assert!(ambiguous_with.is_empty());
        }
        other => panic!("expected Internal, got {other:?}"),
    }
}
