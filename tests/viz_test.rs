//! The 3D map generator: HTML structure, embedded JSON payload, coordinate
//! sanity, and degenerate-input behavior.

use gigagraph::indexer::{Index, build_index};
use gigagraph::viz;
use serde_json::Value;
use std::path::Path;

fn fixture_index() -> Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/endpoints");
    build_index(&root, true).expect("index build failed")
}

fn extract_payload(html: &str) -> Value {
    let open = "<script type=\"application/json\" id=\"graph-data\">";
    let start = html.find(open).expect("data script tag present") + open.len();
    let end = html[start..].find("</script>").expect("data script closed") + start;
    serde_json::from_str(html[start..end].trim()).expect("embedded payload is valid JSON")
}

#[test]
fn map_generates_with_expected_nodes() {
    let index = fixture_index();
    let html = viz::generate_html(&index);

    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<canvas id=\"gl\">"));
    assert!(html.contains("<noscript>"));
    assert!(!html.contains("__GRAPH_DATA__"), "placeholder replaced");

    let payload = extract_payload(&html);
    let nodes = payload["nodes"].as_array().expect("nodes array");
    let expected = index
        .graph
        .functions
        .iter()
        .filter(|f| !f.is_toplevel)
        .count();
    assert_eq!(nodes.len(), expected, "one node per non-toplevel function");
    assert_eq!(payload["meta"]["shown"].as_u64(), Some(expected as u64));
    assert_eq!(payload["meta"]["total"].as_u64(), Some(expected as u64));
    assert_eq!(payload["meta"]["capped"].as_bool(), Some(false));

    let ids: std::collections::HashSet<u64> = nodes
        .iter()
        .map(|n| n["id"].as_u64().expect("numeric id"))
        .collect();
    let mut saw_endpoint = false;
    let mut names = Vec::new();
    for n in nodes {
        // Coordinates present, numeric, finite (serde_json would emit null
        // for NaN — that must never happen).
        for k in ["x", "y", "z"] {
            let v = n[k].as_f64().unwrap_or_else(|| panic!("{k} is a number"));
            assert!(v.is_finite(), "{k} finite");
        }
        for k in ["name", "qn", "file", "lang", "sig"] {
            assert!(n[k].is_string(), "{k} is a string");
        }
        assert!(n["line"].is_u64());
        assert!(n["sz"].is_u64());
        assert!(n["ep"].is_boolean());
        saw_endpoint |= n["ep"].as_bool() == Some(true);
        // Neighbor/similar lists only reference nodes that exist in the map.
        for list in ["sim", "out", "in"] {
            for id in n[list].as_array().expect("id list") {
                assert!(ids.contains(&id.as_u64().unwrap()), "{list} id in map");
            }
        }
        names.push(n["name"].as_str().unwrap().to_string());
        assert_ne!(
            n["name"].as_str(),
            Some("(toplevel)"),
            "synthetics excluded"
        );
    }
    assert!(saw_endpoint, "endpoint handlers are flagged");
    assert!(
        names.iter().any(|n| n == "getUser"),
        "express handler getUser present; got: {names:?}"
    );
}

#[test]
fn map_generation_is_deterministic() {
    let index = fixture_index();
    assert_eq!(viz::generate_html(&index), viz::generate_html(&index));
}

#[test]
fn degenerate_tiny_project_has_no_nan() {
    let dir = std::env::temp_dir().join(format!("gigagraph-viz-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("solo.js"), "function lonely() { return 1; }\n").unwrap();

    let index = build_index(&dir, true).expect("tiny index builds");
    let html = viz::generate_html(&index);
    let payload = extract_payload(&html);
    for n in payload["nodes"].as_array().unwrap() {
        for k in ["x", "y", "z"] {
            let v = n[k].as_f64().expect("coordinate is a number, not null");
            assert!(v.is_finite());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_map_writes_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/endpoints");
    let out = std::env::temp_dir().join(format!("gigagraph-viz-out-{}.html", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let written = viz::write_map(&root, Some(&out)).expect("write_map succeeds");
    assert!(written.is_absolute());
    let html = std::fs::read_to_string(&written).unwrap();
    assert!(html.contains("graph-data"));
    let _ = std::fs::remove_file(&out);
}
