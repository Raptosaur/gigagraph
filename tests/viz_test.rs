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
fn endpoint_nodes_and_client_edges_embedded() {
    let index = fixture_index();
    let html = viz::generate_html(&index);
    let payload = extract_payload(&html);

    let node_ids: std::collections::HashSet<u64> = payload["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();

    // Endpoint nodes: one row per detected endpoint (fixture is small, no cap).
    let eps = payload["eps"].as_array().expect("eps array");
    assert_eq!(eps.len(), index.graph.endpoints.endpoints.len());
    assert!(!eps.is_empty(), "endpoints fixture must yield endpoint nodes");
    let ep_ids: std::collections::HashSet<u64> = eps
        .iter()
        .map(|e| e["id"].as_u64().expect("endpoint id"))
        .collect();
    for e in eps {
        for k in ["kind", "method", "path", "framework", "conf", "file"] {
            assert!(e[k].is_string(), "endpoint field {k} is a string");
        }
        assert!(e["line"].is_u64());
        assert!(e["sv"].is_u64());
        assert!(e["m"].is_u64());
        for k in ["x", "y", "z"] {
            let v = e[k].as_f64().unwrap_or_else(|| panic!("{k} is a number"));
            assert!(v.is_finite(), "endpoint {k} finite");
        }
        assert!(matches!(e["conf"].as_str(), Some("high" | "heuristic")));
        // Resolved handlers must point at function nodes present in the map.
        if let Some(h) = e["h"].as_u64() {
            assert!(node_ids.contains(&h), "handler id in node set");
        }
    }
    // At least one endpoint has its handler wired to a map node.
    assert!(
        eps.iter().any(|e| e["h"].is_u64()),
        "at least one endpoint has a resolved handler"
    );

    // Client-call -> endpoint match rows.
    let cc = payload["cc"].as_array().expect("cc array");
    assert!(!cc.is_empty(), "correlated client calls must be embedded");
    assert_eq!(cc.len() as u64, payload["meta"]["matches"].as_u64().unwrap());
    for f in cc {
        assert!(
            ep_ids.contains(&f["to"].as_u64().expect("to id")),
            "cc target is an embedded endpoint"
        );
        assert!(f["fsv"].is_u64());
        assert!(matches!(f["conf"].as_str(), Some("high" | "heuristic")));
        for k in ["kind", "method", "url", "lib", "file"] {
            assert!(f[k].is_string(), "cc field {k} is a string");
        }
        if let Some(from) = f["from"].as_u64() {
            assert!(node_ids.contains(&from), "cc source is a map node");
        }
    }
}

#[test]
fn single_service_fixture_defaults_to_api_panel() {
    let index = fixture_index();
    let payload = extract_payload(&viz::generate_html(&index));
    let meta = &payload["meta"];
    assert_eq!(meta["multi"].as_bool(), Some(false), "flat fixture is not a monorepo");
    assert_eq!(meta["mode"].as_str(), Some("api"), "single-service opens on the API panel");
    let services = meta["services"].as_array().expect("services array");
    assert!(!services.is_empty());
    for s in services {
        assert!(s["name"].is_string());
        for k in ["files", "functions", "endpoints"] {
            assert!(s[k].is_u64(), "service stat {k}");
        }
    }
}

#[test]
fn monorepo_fixture_groups_services_and_defaults_to_flow() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/monorepo");
    let index = build_index(&root, true).expect("monorepo index build failed");
    let html = viz::generate_html(&index);
    // Determinism holds on the monorepo payload too.
    assert_eq!(html, viz::generate_html(&index));
    let payload = extract_payload(&html);
    let meta = &payload["meta"];

    let names: Vec<&str> = meta["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    for svc in ["web", "pysvc", "gosvc"] {
        assert!(names.contains(&svc), "service group {svc} present; got {names:?}");
    }
    assert_eq!(meta["multi"].as_bool(), Some(true), ">=2 endpoint groups => multi-service");
    assert_eq!(meta["mode"].as_str(), Some("flow"), "monorepo opens on the flow view");

    // Endpoints exist in >= 2 distinct service groups.
    let eps = payload["eps"].as_array().unwrap();
    let ep_svcs: std::collections::HashSet<u64> =
        eps.iter().map(|e| e["sv"].as_u64().unwrap()).collect();
    assert!(ep_svcs.len() >= 2, "endpoints span services: {ep_svcs:?}");

    // At least one correlated client call crosses a service boundary
    // (web/src/client.ts -> pysvc + gosvc endpoints).
    let ep_sv: std::collections::HashMap<u64, u64> = eps
        .iter()
        .map(|e| (e["id"].as_u64().unwrap(), e["sv"].as_u64().unwrap()))
        .collect();
    let cc = payload["cc"].as_array().unwrap();
    assert!(!cc.is_empty(), "monorepo fixture has correlated client calls");
    assert!(
        cc.iter().any(|f| {
            let to_sv = ep_sv[&f["to"].as_u64().unwrap()];
            f["fsv"].as_u64().unwrap() != to_sv
        }),
        "at least one cross-service client->endpoint edge"
    );
}

#[test]
fn service_grouping_conventions() {
    assert_eq!(viz::service_of("web/src/api.ts"), "web");
    assert_eq!(viz::service_of("apps/web/src/api.ts"), "apps/web");
    assert_eq!(viz::service_of("packages/ui/index.ts"), "packages/ui");
    assert_eq!(viz::service_of("apps/readme.md"), "apps");
    assert_eq!(viz::service_of("main.rs"), "(root)");
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
