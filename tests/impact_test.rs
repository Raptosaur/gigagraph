//! Blast-radius traversal and affected-test derivation over a small
//! multi-hop fixture: util <- service <- Flask endpoint <- HTTP client,
//! with pytest-style and #[test]-style tests hanging off the chain.

use gigagraph::impact::{affected_tests, blast_radius};
use gigagraph::indexer::build_index;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/impact");
    build_index(&root, true).expect("index build failed")
}

fn fn_id(g: &gigagraph::graph::GigaGraph, name: &str) -> u32 {
    let ids = g
        .name_index
        .get(name)
        .unwrap_or_else(|| panic!("no fn {name}"));
    let real: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|&id| !g.functions[id as usize].is_toplevel)
        .collect();
    assert_eq!(real.len(), 1, "ambiguous {name}");
    real[0]
}

#[test]
fn classifies_tests() {
    let index = index();
    let g = &index.graph;
    // Decoration-based (#[test]), name+path based (pytest), helper-in-test-file.
    assert!(g.functions[fn_id(g, "doubles") as usize].is_test);
    assert!(g.functions[fn_id(g, "test_load_widget") as usize].is_test);
    assert!(g.functions[fn_id(g, "make_fixture_id") as usize].is_test);
    // Production code stays unflagged.
    assert!(!g.functions[fn_id(g, "parse_widget") as usize].is_test);
    assert!(!g.functions[fn_id(g, "get_widget") as usize].is_test);
}

#[test]
fn walks_caller_closure_and_jumps_the_endpoint() {
    let index = index();
    let g = &index.graph;
    let seed = fn_id(g, "parse_widget");
    let res = blast_radius(g, &[seed], 10);

    let depth_of = |name: &str| res.impacted.get(&fn_id(g, name)).map(|i| i.depth);
    assert_eq!(depth_of("load_widget"), Some(1));
    assert_eq!(depth_of("get_widget"), Some(2));
    // The Flask route's correlated requests.get caller is pulled across the
    // HTTP boundary.
    let fetch = res
        .impacted
        .get(&fn_id(g, "fetch_widget"))
        .expect("endpoint jump missing");
    assert_eq!(fetch.depth, 3);
    assert_eq!(fetch.via, "endpoint-client");
    // Unrelated code stays out.
    assert!(!res.impacted.contains_key(&fn_id(g, "unrelated_util")));
    assert!(!res.impacted.contains_key(&fn_id(g, "double")));

    // max_depth is honored.
    let shallow = blast_radius(g, &[seed], 1);
    assert!(shallow.impacted.contains_key(&fn_id(g, "load_widget")));
    assert!(!shallow.impacted.contains_key(&fn_id(g, "get_widget")));
}

#[test]
fn derives_affected_tests() {
    let index = index();
    let g = &index.graph;

    let seed = fn_id(g, "parse_widget");
    let res = blast_radius(g, &[seed], 10);
    let tests = affected_tests(g, &[seed], &res);
    let names: Vec<&str> = tests
        .iter()
        .map(|(id, _, _)| g.functions[*id as usize].name.as_str())
        .collect();
    assert!(names.contains(&"test_parse"));
    assert!(names.contains(&"test_load_widget"));
    assert!(!names.contains(&"doubles"));

    // Rust: #[test] reached through a direct call edge.
    let seed = fn_id(g, "double");
    let res = blast_radius(g, &[seed], 10);
    let tests = affected_tests(g, &[seed], &res);
    assert!(
        tests
            .iter()
            .any(|(id, _, _)| g.functions[*id as usize].name == "doubles")
    );
}
