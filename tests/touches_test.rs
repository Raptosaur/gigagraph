//! Touch memory: ring caps (global + per-file), filtering, path
//! normalization, and the record_touch/recent_touches tool roundtrip.

use gigagraph::api::AppState;
use gigagraph::touches::{self, MAX_GLOBAL, MAX_PER_FILE};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Unique scratch directory under the system temp dir (no tempfile crate).
fn scratch(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "gigagraph_touches_{tag}_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn record(root: &PathBuf, files: &[&str], why: &str) {
    let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
    touches::record_touch(root, &files, why, "test").unwrap();
}

#[test]
fn global_cap_keeps_newest_250() {
    let root = scratch("global");
    let extra = 10;
    for i in 0..MAX_GLOBAL + extra {
        // Unique file per entry so the per-file cap never kicks in.
        record(&root, &[&format!("src/f{i}.rs")], &format!("entry-{i}"));
    }
    let all = touches::read_touches(&root).unwrap();
    assert_eq!(all.len(), MAX_GLOBAL);
    // Oldest `extra` entries were dropped; order (oldest first) preserved.
    assert_eq!(all.first().unwrap().why, format!("entry-{extra}"));
    assert_eq!(
        all.last().unwrap().why,
        format!("entry-{}", MAX_GLOBAL + extra - 1)
    );
    // Lock is released, ring file exists.
    assert!(root.join(".gigagraph/touches.jsonl").exists());
    assert!(!root.join(".gigagraph/touches.lock").exists());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn per_file_cap_keeps_newest_10_without_evicting_others() {
    let root = scratch("perfile");
    for i in 0..12 {
        record(&root, &["src/hot.rs"], &format!("hot-{i}"));
    }
    record(&root, &["src/cold.rs"], "cold-survives");
    for i in 12..15 {
        record(&root, &["src/hot.rs"], &format!("hot-{i}"));
    }
    let all = touches::read_touches(&root).unwrap();
    let hot: Vec<&str> = all
        .iter()
        .filter(|t| t.files.iter().any(|f| f == "src/hot.rs"))
        .map(|t| t.why.as_str())
        .collect();
    assert_eq!(hot.len(), MAX_PER_FILE);
    // The newest 10 mentions survive (5..15), oldest extras dropped.
    assert_eq!(hot.first().unwrap(), &"hot-5");
    assert_eq!(hot.last().unwrap(), &"hot-14");
    // Trimming hot entries must not evict unrelated files' entries.
    assert!(all.iter().any(|t| t.why == "cold-survives"));
    assert_eq!(all.len(), MAX_PER_FILE + 1);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn recent_filters_by_file_newest_first() {
    let root = scratch("filter");
    record(&root, &["src/a.rs"], "a1");
    record(&root, &["src/b.rs"], "b1");
    record(&root, &["src/a.rs"], "a2");
    record(&root, &["src/a.rs", "src/b.rs"], "both");

    // No file: everything, newest first.
    let all = touches::recent(&root, None, 50).unwrap();
    let whys: Vec<&str> = all.iter().map(|t| t.why.as_str()).collect();
    assert_eq!(whys, ["both", "a2", "b1", "a1"]);

    // Exact repo-relative filter (multi-file entries count as mentions).
    let a = touches::recent(&root, Some("src/a.rs"), 50).unwrap();
    let whys: Vec<&str> = a.iter().map(|t| t.why.as_str()).collect();
    assert_eq!(whys, ["both", "a2", "a1"]);

    // Suffix convenience: `a.rs` matches `src/a.rs`.
    let a = touches::recent(&root, Some("a.rs"), 50).unwrap();
    assert_eq!(a.len(), 3);

    // Limit applies after filtering.
    let a = touches::recent(&root, Some("src/a.rs"), 2).unwrap();
    let whys: Vec<&str> = a.iter().map(|t| t.why.as_str()).collect();
    assert_eq!(whys, ["both", "a2"]);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn absolute_paths_normalize_to_repo_relative() {
    let root = scratch("norm");
    // Absolute under the raw root path (file need not exist).
    let abs = root.join("src/lib.rs");
    record(&root, &[abs.to_str().unwrap()], "via-raw-root");
    // Absolute under the canonicalized root (e.g. /private/var on macOS).
    let canon = root.canonicalize().unwrap().join("src/lib.rs");
    record(&root, &[canon.to_str().unwrap()], "via-canon-root");
    // Relative with ./ prefix.
    record(&root, &["./src/lib.rs"], "via-dot");

    let all = touches::read_touches(&root).unwrap();
    assert_eq!(all.len(), 3);
    for t in &all {
        assert_eq!(t.files, ["src/lib.rs"], "entry {}", t.why);
    }
    // All three count against the same per-file bucket.
    let hits = touches::recent(&root, Some("src/lib.rs"), 50).unwrap();
    assert_eq!(hits.len(), 3);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn dispatch_roundtrip_and_validation() {
    let root = scratch("dispatch");
    let mut state = AppState::new(root.clone());

    // CLI and MCP share this exact append path (dispatch -> record_touch).
    let out = state
        .dispatch(
            "record_touch",
            &json!({ "files": ["src/api.rs"], "why": "add touch tools", "agent": "tester" }),
        )
        .unwrap();
    assert_eq!(out["recorded"]["why"], "add touch tools");
    assert_eq!(out["recorded"]["agent"], "tester");
    assert_eq!(out["recorded"]["files"], json!(["src/api.rs"]));
    assert_eq!(out["total_entries"], 1);
    assert_eq!(out["file_counts"]["src/api.rs"], 1);
    assert!(out["recorded"]["ts"].as_u64().unwrap() > 0);

    // Default agent.
    let out = state
        .dispatch(
            "record_touch",
            &json!({ "files": ["src/mcp.rs"], "why": "second" }),
        )
        .unwrap();
    assert_eq!(out["recorded"]["agent"], "unknown");
    assert_eq!(out["total_entries"], 2);

    // recent_touches: global then filtered, newest first, with the
    // not-authoritative note.
    let out = state.dispatch("recent_touches", &json!({})).unwrap();
    let rows = out["touches"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["why"], "second");
    assert!(out["note"].as_str().unwrap().contains("git log"));

    let out = state
        .dispatch("recent_touches", &json!({ "file": "api.rs", "limit": 1 }))
        .unwrap();
    let rows = out["touches"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["why"], "add touch tools");

    // Rejections: empty/missing files, empty why.
    for bad in [
        json!({ "files": [], "why": "x" }),
        json!({ "files": ["   "], "why": "x" }),
        json!({ "why": "x" }),
        json!({ "files": ["src/api.rs"], "why": "  " }),
        json!({ "files": ["src/api.rs"] }),
    ] {
        assert!(
            state.dispatch("record_touch", &bad).is_err(),
            "expected rejection for {bad}"
        );
    }
    // Nothing was appended by the rejected calls.
    let out = state.dispatch("recent_touches", &json!({})).unwrap();
    assert_eq!(out["touches"].as_array().unwrap().len(), 2);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn recent_touches_limit_is_clamped_to_50() {
    let root = scratch("clamp");
    for i in 0..60 {
        record(&root, &[&format!("src/f{i}.rs")], &format!("e{i}"));
    }
    let mut state = AppState::new(root.clone());
    let out = state
        .dispatch("recent_touches", &json!({ "limit": 500 }))
        .unwrap();
    assert_eq!(out["touches"].as_array().unwrap().len(), 50);
    let out: Value = state.dispatch("recent_touches", &json!({})).unwrap();
    assert_eq!(out["touches"].as_array().unwrap().len(), 10);
    std::fs::remove_dir_all(&root).ok();
}
