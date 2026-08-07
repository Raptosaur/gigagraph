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
fn sam_legacy_forms() {
    let idx = index();
    let g = &idx.graph;

    // Single-quoted Transform + Method: ANY event.
    let any = ep(g, "sam", "ANY", "/legacy/items");
    assert_eq!(g.files[any.file_id as usize].path, "sam_legacy.yaml");
    assert_eq!(
        handler_loc(g, any),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );

    // AWS::Serverless::Api DefinitionBody inline swagger: paths/methods walk,
    // lambda through x-amazon-apigateway-integration's Fn::Sub uri.
    let sw_get = ep(g, "sam", "GET", "/swagger/pets");
    assert_eq!(
        handler_loc(g, sw_get),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );
    let sw_post = ep(g, "sam", "POST", "/swagger/pets");
    assert_eq!(
        handler_loc(g, sw_post),
        Some(("src/handlers/items.js".into(), "handler".into()))
    );
}

#[test]
fn cfn_long_form_intrinsics() {
    let idx = index();
    let g = &idx.graph;

    // ApiGatewayV2 route via long-form {Ref}/Fn::Join/Fn::Sub.
    let del = ep(g, "cloudformation", "DELETE", "/archive/{*}");
    assert_eq!(g.files[del.file_id as usize].path, "cfn_long.yaml");
    assert_eq!(
        handler_loc(g, del),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // REST chain via long-form Fn::GetAtt RootResourceId + {Ref} ResourceId;
    // integration Uri as Fn::Join with a nested Fn::GetAtt part.
    let get = ep(g, "cloudformation", "GET", "/archive");
    assert_eq!(
        handler_loc(g, get),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );
}

#[test]
fn serverless_legacy_forms() {
    let idx = index();
    let g = &idx.graph;

    // v1 shorthand "GET legacy": no leading slash in the event string.
    let legacy = ep(g, "serverless", "GET", "/legacy");
    assert_eq!(
        handler_loc(g, legacy),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // {proxy+} greedy segment folds to {*}; integration/request keys ignored.
    let proxy = ep(g, "serverless", "ANY", "/assets/{*}");
    assert_eq!(proxy.path_raw, "/assets/{proxy+}");

    // httpApi '*' catch-all.
    let star = g
        .endpoints
        .endpoints
        .iter()
        .find(|e| e.framework == "serverless" && e.path_raw == "$default")
        .expect("httpApi '*' catch-all endpoint");
    assert_eq!(star.path_norm, "/{*}");

    // v1 `service: {name: ...}` object form still passes corroboration.
    let old = ep(g, "serverless", "GET", "/old-users");
    assert_eq!(g.files[old.file_id as usize].path, "legacy/serverless.yml");
    assert_eq!(
        handler_loc(g, old),
        Some(("src/handlers/users.js".into(), "create".into()))
    );
}

#[test]
fn terraform_legacy_forms() {
    let idx = index();
    let g = &idx.graph;

    // HCL1-flavored quoted interpolations parse; route_key still extracts
    // and the "${...}" traversal strings resolve through tpl_ref.
    let old = ep(g, "terraform", "GET", "/old");
    assert_eq!(g.files[old.file_id as usize].path, "legacy.tf");
    assert_eq!(
        handler_loc(g, old),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // Quick-create aws_apigatewayv2_api: inline route_key + target.
    let quick = ep(g, "terraform", "POST", "/quick");
    assert_eq!(
        handler_loc(g, quick),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // Pre-4.x literal zip filename (no archive_file): base dir falls back to
    // the .tf dir and the handler stem resolves by path suffix.
    let zip = ep(g, "lambda", "ANY", "/lambda/legacy_zip");
    assert_eq!(zip.path_raw, "lambda:legacy_zip");
    assert_eq!(
        handler_loc(g, zip),
        Some(("src/handlers/report.py".into(), "lambda_handler".into()))
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
