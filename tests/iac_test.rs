//! IaC endpoint scanning over the iac fixture: SAM, raw CloudFormation,
//! serverless.yml, and Terraform declarations with handler resolution into
//! the indexed JS/TS/Python sources.

use gigagraph::api::AppState;
use gigagraph::endpoints::{ApiKind, Endpoint};
use gigagraph::graph::GigaGraph;
use gigagraph::indexer::{Index, build_index};
use serde_json::json;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iac")
}

fn index() -> Index {
    build_index(&root(), true).unwrap()
}

fn ep<'a>(g: &'a GigaGraph, framework: &str, method: &str, norm: &str) -> &'a Endpoint {
    g.endpoints
        .endpoints
        .iter()
        .find(|e| e.framework == framework && e.method.as_str() == method && e.path_norm == norm)
        .unwrap_or_else(|| {
            panic!(
                "missing endpoint {framework} {method} {norm}; have: {:?}",
                g.endpoints
                    .endpoints
                    .iter()
                    .map(|e| format!("{} {} {}", e.framework, e.method.as_str(), e.path_norm))
                    .collect::<Vec<_>>()
            )
        })
}

fn handler_loc(g: &GigaGraph, e: &Endpoint) -> Option<(String, String)> {
    e.handler.map(|h| {
        let f = &g.functions[h as usize];
        (g.files[f.file_id as usize].path.clone(), f.name.clone())
    })
}

#[test]
fn sam_template_endpoints() {
    let idx = index();
    let g = &idx.graph;

    let orders = ep(g, "sam", "GET", "/orders");
    assert_eq!(g.files[orders.file_id as usize].path, "template.yaml");
    assert_eq!(
        handler_loc(g, orders),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // esbuild metadata redirects the handler stem to the entry point.
    let esbuild = ep(g, "sam", "POST", "/orders/{*}");
    assert_eq!(
        handler_loc(g, esbuild),
        Some(("src/app.ts".into(), "handler".into()))
    );

    // Schedule-only function surfaces as a lambda entry point (handler
    // resolved, otherwise it would be dropped).
    let worker = ep(g, "lambda", "ANY", "/lambda/workerfunction");
    assert_eq!(worker.path_raw, "lambda:WorkerFunction");
    assert_eq!(
        handler_loc(g, worker),
        Some(("src/handlers/report.py".into(), "lambda_handler".into()))
    );
}

#[test]
fn cloudformation_endpoints() {
    let idx = index();
    let g = &idx.graph;

    // ApiGatewayV2 route -> integration -> lambda with CDK asset metadata.
    let items = ep(g, "cloudformation", "GET", "/items/{*}");
    assert_eq!(g.files[items.file_id as usize].path, "cfn.yaml");
    assert_eq!(
        handler_loc(g, items),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );

    // Lambda without local code: route detected, handler honestly None.
    let external = ep(g, "cloudformation", "POST", "/external");
    assert_eq!(external.handler, None);

    // REST resource chain: root -> users -> {userId}.
    let del = ep(g, "cloudformation", "DELETE", "/users/{*}");
    assert_eq!(del.path_raw, "/users/{userId}");
    assert_eq!(
        handler_loc(g, del),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );

    // AppSync resolver -> Graphql op, lambda-backed through the data source.
    let gql = ep(g, "appsync", "ANY", "/query.getuser");
    assert_eq!(gql.kind, ApiKind::Graphql);
    assert_eq!(gql.path_raw, "Query.getUser");
    assert_eq!(
        handler_loc(g, gql),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );
}

#[test]
fn serverless_yml_endpoints() {
    let idx = index();
    let g = &idx.graph;

    // http object event; path gains its missing leading slash.
    let create = ep(g, "serverless", "POST", "/users");
    assert_eq!(g.files[create.file_id as usize].path, "serverless.yml");
    assert_eq!(
        handler_loc(g, create),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // httpApi shorthand string + per-function python runtime override.
    let reports = ep(g, "serverless", "GET", "/reports");
    assert_eq!(
        handler_loc(g, reports),
        Some(("src/handlers/report.py".into(), "lambda_handler".into()))
    );

    // Function URL: catch-all.
    let url = ep(g, "serverless", "ANY", "/{*}");
    assert_eq!(
        handler_loc(g, url),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // serverless-appsync plugin resolver key.
    let gql = ep(g, "serverless-appsync", "ANY", "/query.getreport");
    assert_eq!(gql.kind, ApiKind::Graphql);
    assert_eq!(gql.handler, None);
}

#[test]
fn terraform_endpoints() {
    let idx = index();
    let g = &idx.graph;

    // v2 route -> integration -> lambda -> archive_file source_dir.
    let put = ep(g, "terraform", "PUT", "/orders/{*}");
    assert_eq!(g.files[put.file_id as usize].path, "main.tf");
    assert_eq!(
        handler_loc(g, put),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // REST chain via parent_id traversals, python handler.
    let get = ep(g, "terraform", "GET", "/reports/{*}");
    assert_eq!(get.path_raw, "/reports/{reportId}");
    assert_eq!(
        handler_loc(g, get),
        Some(("src/handlers/report.py".into(), "lambda_handler".into()))
    );

    // Lambda function URL.
    let url = ep(g, "terraform", "ANY", "/{*}");
    assert_eq!(
        handler_loc(g, url),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // AppSync resolver via aws_appsync_datasource lambda_config.
    let gql = ep(g, "terraform", "ANY", "/query.getorder");
    assert_eq!(gql.kind, ApiKind::Graphql);
    assert_eq!(
        handler_loc(g, gql),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // Route-less lambda surfaces only because its handler resolved.
    let cron = ep(g, "lambda", "ANY", "/lambda/cron");
    assert_eq!(cron.path_raw, "lambda:cron");
    assert_eq!(
        handler_loc(g, cron),
        Some(("src/handlers/users.js".into(), "create".into()))
    );
}

#[test]
fn endpoint_ids_sequential() {
    let idx = index();
    for (i, e) in idx.graph.endpoints.endpoints.iter().enumerate() {
        assert_eq!(e.id, i as u32, "endpoint ids must stay sequential");
    }
}

#[test]
fn list_endpoints_tool_roundtrip() {
    let mut state = AppState::new(root());
    let out = state
        .dispatch("list_endpoints", &json!({"framework": "terraform"}))
        .expect("tool failed");
    let rows = out["endpoints"].as_array().unwrap();
    assert!(rows.len() >= 4, "{out}");
    assert!(rows.iter().all(|r| r["framework"] == "terraform"), "{out}");
    let put = rows
        .iter()
        .find(|r| r["method"] == "PUT" && r["normalized"] == "/orders/{*}")
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        put["handler"].as_str().unwrap().ends_with("::handler"),
        "{out}"
    );
    assert_eq!(put["kind"], "http");
}
