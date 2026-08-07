//! AWS CDK + AppSync endpoint detection over the tests/fixtures/cdk tree.

use gigagraph::endpoints::{ApiKind, Endpoint, HttpMethod};
use gigagraph::indexer::build_index;
use gigagraph::types::Confidence;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cdk");
    build_index(&root, true).expect("index build failed")
}

/// Find an HTTP endpoint by (method, normalized path, declaring-file suffix).
/// The file filter matters: the TS function URL and the Python LambdaRestApi
/// both normalize to ANY /{*}.
fn find<'a>(
    idx: &'a gigagraph::indexer::Index,
    method: HttpMethod,
    norm: &str,
    file_sfx: &str,
) -> &'a Endpoint {
    let ep = &idx.graph.endpoints;
    ep.endpoints
        .iter()
        .find(|e| {
            e.method == method
                && e.path_norm == norm
                && idx.graph.files[e.file_id as usize].path.ends_with(file_sfx)
        })
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| {
                    format!(
                        "{} {} ({}) in {}",
                        e.method.as_str(),
                        e.path_norm,
                        e.framework,
                        idx.graph.files[e.file_id as usize].path
                    )
                })
                .collect();
            panic!(
                "no endpoint {} {norm} in *{file_sfx}; got: {all:#?}",
                method.as_str()
            )
        })
}

/// Find an RPC-style endpoint by (kind, normalized op).
fn find_rpc<'a>(idx: &'a gigagraph::indexer::Index, kind: ApiKind, norm: &str) -> &'a Endpoint {
    let ep = &idx.graph.endpoints;
    ep.endpoints
        .iter()
        .find(|e| e.kind == kind && e.path_norm == norm)
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| format!("{:?} {} ({})", e.kind, e.path_norm, e.framework))
                .collect();
            panic!("no {kind:?} endpoint {norm}; got: {all:#?}")
        })
}

fn handler_name<'a>(idx: &'a gigagraph::indexer::Index, e: &Endpoint) -> &'a str {
    let h = e.handler.unwrap_or_else(|| {
        panic!("endpoint {} {} has no handler", e.method.as_str(), e.path_norm)
    });
    &idx.graph.functions[h as usize].name
}

#[test]
fn detects_ts_cdk_stack() {
    let idx = index();

    // REST API: linear addResource chain joined in byte order.
    let users = find(&idx, HttpMethod::Get, "/users", "stack.ts");
    assert_eq!(users.framework, "cdk");
    assert_eq!(users.confidence, Confidence::Heuristic);
    // File-scope lambda heuristic: handler resolved through the fromAsset
    // dir + Lambda's index.handler convention (TS constructor props are
    // invisible to the extractor).
    assert_eq!(handler_name(&idx, users), "handler");
    let show = find(&idx, HttpMethod::Delete, "/users/{*}", "stack.ts");
    assert_eq!(show.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, show), "handler");

    // Chained resourceForPath('/books/{bookId}').addMethod('POST', ...):
    // exact path via same-start-byte containment; still Heuristic because
    // the handler is borrowed from the file-scope lambda.
    let books = find(&idx, HttpMethod::Post, "/books/{*}", "stack.ts");
    assert_eq!(books.framework, "cdk");
    assert_eq!(books.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, books), "handler");

    // HTTP API v2 addRoutes: TS object-literal array members are not
    // harvested, so methods degrade honestly to ANY (path stays literal).
    let orders = find(&idx, HttpMethod::Any, "/orders", "stack.ts");
    assert_eq!(orders.framework, "cdk");
    assert_eq!(handler_name(&idx, orders), "handler");

    // Lambda function URL: one HTTPS entry point, any method/path.
    let furl = find(&idx, HttpMethod::Any, "/{*}", "stack.ts");
    assert_eq!(furl.framework, "cdk");
    assert_eq!(furl.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, furl), "handler");

    // AppSync resolver -> Graphql op "Query.getUser".
    let gql = find_rpc(&idx, ApiKind::Graphql, "/query.getuser");
    assert_eq!(gql.framework, "cdk-appsync");
    assert_eq!(gql.method, HttpMethod::Any);
    assert_eq!(gql.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, gql), "handler");

    // The handler function lives in the asset dir's index.ts.
    let hid = users.handler.unwrap();
    assert_eq!(
        idx.graph.files[idx.graph.functions[hid as usize].file_id as usize].path,
        "lambda/index.ts"
    );
}

#[test]
fn detects_py_cdk_stack() {
    let idx = index();

    // REST API add_resource chain.
    let pets = find(&idx, HttpMethod::Get, "/pets", "stack.py");
    assert_eq!(pets.framework, "cdk");
    assert_eq!(pets.confidence, Confidence::Heuristic);
    // handler="app.lambda_handler" + Code.from_asset("src") resolved to the
    // indexed src/app.py function, attached via the file-scope heuristic.
    assert_eq!(handler_name(&idx, pets), "lambda_handler");
    let pet = find(&idx, HttpMethod::Put, "/pets/{*}", "stack.py");
    assert_eq!(handler_name(&idx, pet), "lambda_handler");

    // LambdaRestApi: proxy-all.
    let proxy = find(&idx, HttpMethod::Any, "/{*}", "stack.py");
    assert_eq!(proxy.framework, "cdk");
    assert_eq!(proxy.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, proxy), "lambda_handler");

    // HTTP API v2 add_routes: Python kwargs surface the HttpMethod idents,
    // one endpoint per method.
    let get_orders = find(&idx, HttpMethod::Get, "/orders", "stack.py");
    assert_eq!(get_orders.framework, "cdk");
    assert_eq!(get_orders.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, get_orders), "lambda_handler");
    find(&idx, HttpMethod::Post, "/orders", "stack.py");

    // AppSync: create_resolver kwargs and the Resolver construct.
    let add_pet = find_rpc(&idx, ApiKind::Graphql, "/mutation.addpet");
    assert_eq!(add_pet.framework, "cdk-appsync");
    assert_eq!(handler_name(&idx, add_pet), "lambda_handler");
    let list_pets = find_rpc(&idx, ApiKind::Graphql, "/query.listpets");
    assert_eq!(list_pets.framework, "cdk-appsync");

    // Resolution landed on src/app.py.
    let hid = pets.handler.unwrap();
    assert_eq!(
        idx.graph.files[idx.graph.functions[hid as usize].file_id as usize].path,
        "src/app.py"
    );
}

#[test]
fn multi_lambda_file_gets_no_binding() {
    let idx = index();

    // Two lambda declarations in multi.py: the single-lambda-per-file
    // heuristic must refuse to pick one. Path stays literal -> High.
    let multi = find(&idx, HttpMethod::Get, "/multi", "multi.py");
    assert_eq!(multi.framework, "cdk");
    assert!(multi.handler.is_none(), "ambiguous lambda must not bind");
    assert_eq!(multi.confidence, Confidence::High);
}
