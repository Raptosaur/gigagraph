//! Tier-1 feature enrichment (verb buckets, subwords, transitive effect
//! features) and the blended structural + semantic similarity search.

use gigagraph::api::AppState;
use gigagraph::extract;
use gigagraph::graph::{FileInput, GigaGraph};
use gigagraph::lang;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;

/// Tests share fixtures' on-disk .gigagraph caches; serialize builds.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

/// Build a graph (plus feature bags) straight from in-memory sources.
fn build(files: &[(&str, &str)]) -> (GigaGraph, Vec<FxHashMap<String, u32>>) {
    let inputs: Vec<FileInput> = files
        .iter()
        .map(|(path, src)| {
            let ext = path.rsplit('.').next().unwrap();
            let spec = lang::spec_for_ext(ext).expect("language spec");
            FileInput {
                path: path.to_string(),
                content_hash: 1,
                extracted: extract::extract(spec, src).expect("extraction"),
            }
        })
        .collect();
    GigaGraph::build("/test".to_string(), inputs, "")
}

fn bag_of<'a>(
    g: &GigaGraph,
    features: &'a [FxHashMap<String, u32>],
    name: &str,
) -> &'a FxHashMap<String, u32> {
    let id = g.name_index[name][0] as usize;
    &features[id]
}

#[test]
fn verb_buckets_and_subwords_join_the_bag() {
    let (g, features) = build(&[(
        "svc.ts",
        r#"
export function fetchAccount(id: string) {
    return lookupRecord(id);
}
export function lookupRecord(id: string) {
    return id;
}
"#,
    )]);
    let bag = bag_of(&g, &features, "fetchAccount");
    // Raw features survive untouched.
    assert!(bag.contains_key("call:lookupRecord"), "bag: {bag:?}");
    // Leading verbs of callee/identifier features map into shared buckets.
    assert!(bag.contains_key("vb:READ"), "fetch+lookup -> READ: {bag:?}");
    // Non-verb subwords appear as w: features.
    assert!(bag.contains_key("w:account"), "bag: {bag:?}");
    assert!(bag.contains_key("w:record"), "bag: {bag:?}");
}

#[test]
fn transitive_effects_reach_external_packages_at_depth_two() {
    let (g, features) = build(&[
        (
            "a.ts",
            r#"
import axios from "axios";
export function postJson(url: string, body: unknown) {
    return axios.post(url, body);
}
"#,
        ),
        (
            "b.ts",
            r#"
import { postJson } from "./a";
export function notifyBilling(payload: unknown) {
    return postJson("/billing", payload);
}
"#,
        ),
    ]);

    // Depth 1: postJson touches axios directly.
    let post = bag_of(&g, &features, "postJson");
    assert_eq!(post.get("effpkg:axios"), Some(&4), "bag: {post:?}");

    // Depth 2: notifyBilling -> postJson -> axios. The external package a
    // function ultimately touches is part of its effect signature.
    let notify = bag_of(&g, &features, "notifyBilling");
    assert_eq!(notify.get("effpkg:axios"), Some(&4), "bag: {notify:?}");
    // The depth-1 callee arrives verb-bucketed (post -> EMIT) with weight 4.
    assert!(
        notify.get("eff:EMIT").copied().unwrap_or(0) >= 4,
        "bag: {notify:?}"
    );
    // And the direct callee's verb bucket is present too.
    assert!(notify.contains_key("vb:EMIT"), "bag: {notify:?}");
}

#[test]
fn toplevel_functions_get_no_effect_features() {
    let (g, features) = build(&[(
        "script.ts",
        r#"
import fs from "fs";
fs.readFileSync("x");
"#,
    )]);
    let top = bag_of(&g, &features, "(toplevel)");
    assert!(
        !top.keys().any(|k| k.starts_with("eff")),
        "toplevel must not accumulate effect features: {top:?}"
    );
}

#[test]
fn find_similar_is_deterministic() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/endpoints");

    let run = || {
        let mut state = AppState::new(root.clone());
        let by_function = state
            .dispatch("find_similar", &json!({"function": "getUser"}))
            .expect("find_similar by function");
        let by_snippet = state
            .dispatch(
                "find_similar",
                &json!({
                    "snippet": "function loadAccount(id) { return db.query(id); }",
                    "language": "javascript",
                }),
            )
            .expect("find_similar by snippet");
        (by_function, by_snippet)
    };

    let first = run();
    let second = run();
    assert_eq!(first.0, second.0, "function-mode results must be stable");
    assert_eq!(first.1, second.1, "snippet-mode results must be stable");
    assert!(
        first.0["results"].as_array().is_some_and(|r| !r.is_empty()),
        "expected similarity hits: {}",
        first.0
    );
}

#[test]
fn semantic_blend_prefers_synonym_named_twin() {
    // Three functions with identical structure; query names share meaning
    // (fetch/load -> READ) with one and not the other. The blended score must
    // rank the semantic twin first even though structures tie.
    let (_g, features) = build(&[(
        "twins.ts",
        r#"
export function fetchUser(id: string) {
    return registry.take(id);
}
export function loadUser(id: string) {
    return registry.take(id);
}
export function parseConfig(id: string) {
    return registry.take(id);
}
"#,
    )]);
    let vectors = gigagraph::vector::VectorIndex::build(&features);
    // Ids follow definition order within the single file.
    let fetch = 0u32;
    let query = vectors.vector_of(fetch).unwrap().to_vec();
    let sem = vectors.sem_vector_of(fetch).unwrap().to_vec();
    let hits = vectors.top_k(&query, &sem, 2, Some(fetch));
    assert!(!hits.is_empty());
    assert_eq!(
        hits[0].0, 1,
        "loadUser should outrank parseConfig for fetchUser; hits: {hits:?}"
    );
}
