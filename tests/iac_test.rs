//! IaC endpoint scanning over the iac fixture: SAM, raw CloudFormation,
//! serverless.yml, and Terraform declarations with handler resolution into
//! the indexed JS/TS/Python sources.

use gigagraph::api::AppState;
use gigagraph::endpoints::{ApiKind, Endpoint};
use gigagraph::graph::GigaGraph;
use gigagraph::indexer::{Index, build_index};
use gigagraph::types::Confidence;
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
fn serverless_variable_resolution() {
    let idx = index();
    let g = &idx.graph;

    // ${self:custom.basePath} in the path and ${self:custom.handlers.users}
    // as the handler, both resolved against the same document.
    let profiles = ep(g, "serverless", "GET", "/v1/profiles");
    assert_eq!(profiles.path_raw, "/v1/profiles");
    assert_eq!(profiles.confidence, Confidence::High);
    assert_eq!(
        handler_loc(g, profiles),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // ${opt:stage, 'dev'} -> the default literal.
    let stage = ep(g, "serverless", "GET", "/stage/dev/info");
    assert_eq!(stage.path_raw, "/stage/dev/info");
    assert_eq!(stage.confidence, Confidence::High);

    // Unresolvable self: path stays raw + Heuristic.
    let raw = ep(g, "serverless", "GET", "/raw/{*}/x");
    assert_eq!(raw.path_raw, "/raw/${self:custom.missing}/x");
    assert_eq!(raw.confidence, Confidence::Heuristic);

    // Nested ${..${..}} stays raw + Heuristic, verbatim.
    let nested = ep(g, "serverless", "GET", "/n/{*}/y");
    assert_eq!(nested.path_raw, "/n/${self:custom.${opt:k}}/y");
    assert_eq!(nested.confidence, Confidence::Heuristic);
}

#[test]
fn cloudformation_inline_code() {
    let idx = index();
    let g = &idx.graph;

    // A route whose integration points at a Code.ZipFile lambda: the source
    // is inside the template, so the route row survives with handler
    // honestly None and stays High (the declaration is certain).
    let inline = ep(g, "cloudformation", "GET", "/inline");
    assert_eq!(g.files[inline.file_id as usize].path, "cfn_inline.yaml");
    assert_eq!(inline.handler, None);
    assert_eq!(inline.confidence, Confidence::High);

    // A route-less ZipFile lambda is still surfaced (not silently dropped),
    // handler None.
    let orphan = ep(g, "lambda", "ANY", "/lambda/orphanzip");
    assert_eq!(orphan.path_raw, "lambda:OrphanZip");
    assert_eq!(orphan.handler, None);
    assert_eq!(orphan.confidence, Confidence::Heuristic);
}

#[test]
fn terraform_locals_one_hop() {
    let idx = index();
    let g = &idx.graph;

    // route_key "GET ${local.base}/users" resolves against same-file locals.
    let users = ep(g, "terraform", "GET", "/api/users");
    assert_eq!(g.files[users.file_id as usize].path, "locals.tf");
    assert_eq!(users.path_raw, "/api/users");
    assert_eq!(users.confidence, Confidence::High);
    assert_eq!(
        handler_loc(g, users),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // var.* stays unresolved: raw path + Heuristic.
    let var = ep(g, "terraform", "PUT", "/v/{*}/users");
    assert_eq!(var.path_raw, "/v/${var.stage}/users");
    assert_eq!(var.confidence, Confidence::Heuristic);
}

#[test]
fn serverless_split_functions() {
    let idx = index();
    let g = &idx.graph;

    // Fragment claimed via `functions: - ${file(./functions.yml)}`: routes
    // attach to the fragment file, handlers resolve with the service's
    // base_dir + provider runtime.
    let users = ep(g, "serverless", "GET", "/split-users");
    assert_eq!(g.files[users.file_id as usize].path, "split/functions.yml");
    assert_eq!(
        handler_loc(g, users),
        Some(("src/handlers/users.js".into(), "create".into()))
    );

    // Inline {name: def} entries in the same functions list still work.
    let inline = ep(g, "serverless", "GET", "/split-inline");
    assert_eq!(g.files[inline.file_id as usize].path, "split/serverless.yml");
    assert_eq!(
        handler_loc(g, inline),
        Some(("src/handlers/orders.js".into(), "handler".into()))
    );

    // An unclaimed fragment lookalike contributes nothing.
    assert!(
        g.endpoints
            .endpoints
            .iter()
            .all(|e| e.path_norm != "/unclaimed-route"),
        "unclaimed fragment must stay inert"
    );
}

/// 1-based inclusive start line and exclusive end line of a YAML block: from
/// the `<key>:` line to the next sibling at the same or lower indent.
fn yaml_block_range(src: &str, key: &str) -> (u32, u32) {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| {
            let t = l.trim_start();
            t.strip_prefix(key).is_some_and(|r| r.starts_with(':'))
        })
        .unwrap_or_else(|| panic!("no block {key}"));
    let indent = lines[start].len() - lines[start].trim_start().len();
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| {
            let t = l.trim_start();
            !t.is_empty() && !t.starts_with('#') && (l.len() - t.len()) <= indent
        })
        .map(|(i, _)| i + 1)
        .unwrap_or(lines.len() + 1);
    ((start + 1) as u32, end as u32)
}

/// 1-based inclusive start and end of an HCL block: header line to the first
/// `}` at column 0.
fn tf_block_range(src: &str, header: &str) -> (u32, u32) {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with(header))
        .unwrap_or_else(|| panic!("no block {header}"));
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.starts_with('}'))
        .map(|(i, _)| i + 1)
        .unwrap_or(lines.len());
    ((start + 1) as u32, end as u32)
}

#[test]
fn line_numbers_land_in_declaring_blocks() {
    let idx = index();
    let g = &idx.graph;

    // YAML: the REST method row points inside its logical-id block.
    let cfn = std::fs::read_to_string(root().join("cfn.yaml")).unwrap();
    let (start, end) = yaml_block_range(&cfn, "DeleteUserMethod");
    let del = ep(g, "cloudformation", "DELETE", "/users/{*}");
    assert!(
        del.line >= start && del.line < end,
        "line {} outside DeleteUserMethod block [{start}, {end})",
        del.line
    );

    // SAM: routes from events point at the event id line — inside the
    // declaring function's block but past its header.
    let sam = std::fs::read_to_string(root().join("template.yaml")).unwrap();
    let (fstart, fend) = yaml_block_range(&sam, "OrdersFunction");
    let orders = ep(g, "sam", "GET", "/orders");
    assert!(
        orders.line > fstart && orders.line < fend,
        "line {} outside OrdersFunction event range ({fstart}, {fend})",
        orders.line
    );

    // HCL: anchored to the `resource "type" "name"` header even though the
    // name appears quoted earlier in the file (locals.route_hint).
    let tf = std::fs::read_to_string(root().join("locals.tf")).unwrap();
    let (rstart, rend) =
        tf_block_range(&tf, "resource \"aws_apigatewayv2_route\" \"local_route\"");
    let users = ep(g, "terraform", "GET", "/api/users");
    assert!(
        users.line >= rstart && users.line <= rend,
        "line {} outside local_route block [{rstart}, {rend}]",
        users.line
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
