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
    // File-scope lambda heuristic: the `new lambda.Function` construction's
    // handler prop ('index.handler') resolved against the byte-contained
    // fromAsset dir.
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

    // HTTP API v2 addRoutes: TS MULTI-member methods arrays arrive as
    // `methods`-keyed Idents (harvester depth-3 array reach) -> one row per
    // verb, exactly like the Python kwarg form. Single-member arrays keep
    // their unwrapped shape — see detects_ts_v2_constructs.
    let orders = find(&idx, HttpMethod::Get, "/orders", "stack.ts");
    assert_eq!(orders.framework, "cdk");
    assert_eq!(handler_name(&idx, orders), "handler");
    find(&idx, HttpMethod::Post, "/orders", "stack.ts");
    assert!(
        !idx.graph.endpoints.endpoints.iter().any(|e| {
            e.method == HttpMethod::Any
                && e.path_norm == "/orders"
                && idx.graph.files[e.file_id as usize].path.ends_with("stack.ts")
        }),
        "multi-member methods array must not also widen to ANY"
    );

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
fn detects_ts_v2_constructs() {
    let idx = index();

    // NodejsFunction: `entry` IS the handler source file, `handler` names
    // the export — resolved without any fromAsset call in the file.
    // LambdaRestApi (member form) proxies everything to it: ANY /{*},
    // Heuristic because the handler is the borrowed file-scope lambda.
    let proxy = find(&idx, HttpMethod::Any, "/{*}", "v2app.ts");
    assert_eq!(proxy.framework, "cdk");
    assert_eq!(proxy.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, proxy), "main");
    let hid = proxy.handler.unwrap();
    assert_eq!(
        idx.graph.files[idx.graph.functions[hid as usize].file_id as usize].path,
        "fns/orders.ts"
    );

    // addRoutes with a SINGLE-member methods array: the lone member is
    // unwrapped by the harvester, so the row is per-method, not ANY.
    let reports = find(&idx, HttpMethod::Patch, "/v2reports", "v2app.ts");
    assert_eq!(reports.framework, "cdk");
    assert_eq!(handler_name(&idx, reports), "main");
    assert!(
        !idx.graph
            .endpoints
            .endpoints
            .iter()
            .any(|e| e.method == HttpMethod::Any && e.path_norm == "/v2reports"),
        "single-member methods array must not also widen to ANY"
    );

    // TS L1 CfnRoute: literal 'GET /v2items' routeKey via the
    // Ident-key/Str-value window.
    let items = find(&idx, HttpMethod::Get, "/v2items", "v2app.ts");
    assert_eq!(items.framework, "cdk");
    assert_eq!(items.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, items), "main");

    // TS `new appsync.Resolver({ typeName, fieldName })` L2 construct.
    let status = find_rpc(&idx, ApiKind::Graphql, "/query.orderstatus");
    assert_eq!(status.framework, "cdk-appsync");
    assert_eq!(status.method, HttpMethod::Any);
    assert_eq!(handler_name(&idx, status), "main");
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
fn detects_v1_scoped_package_stack() {
    let idx = index();

    // v1 @aws-cdk/* scoped imports satisfy the same evidence gate; REST
    // addResource chain shape is identical to v2.
    let gizmos = find(&idx, HttpMethod::Get, "/gizmos", "v1app.ts");
    assert_eq!(gizmos.framework, "cdk");
    assert_eq!(handler_name(&idx, gizmos), "handler");

    // @aws-cdk/aws-apigatewayv2 (+ -integrations alpha) addRoutes: same
    // object shape; TS methods array unharvested -> ANY.
    let orders = find(&idx, HttpMethod::Any, "/v1orders", "v1app.ts");
    assert_eq!(orders.framework, "cdk");
    assert_eq!(handler_name(&idx, orders), "handler");

    // v1 AppSync: Schema.fromAsset must NOT be mistaken for a lambda
    // declaration (it would poison the single-lambda binding), and the
    // options-only createResolver arity (no id string) still yields the op.
    let gql = find_rpc(&idx, ApiKind::Graphql, "/query.getgizmo");
    assert_eq!(gql.framework, "cdk-appsync");
    assert_eq!(handler_name(&idx, gql), "handler");
}

#[test]
fn detects_python_l1_cfn_constructs() {
    let idx = index();

    // CfnRoute: literal route_key, kwargs visible in Python.
    let route = find(&idx, HttpMethod::Get, "/legacyitems", "cfnl1.py");
    assert_eq!(route.framework, "cdk");
    assert_eq!(route.confidence, Confidence::Heuristic);
    // CfnFunction handler stem resolved by path-suffix (src/app.py) and
    // borrowed through the file-scope single-lambda binding.
    assert_eq!(handler_name(&idx, route), "lambda_handler");

    // CfnResolver: type_name/field_name kwargs -> Graphql op.
    let gql = find_rpc(&idx, ApiKind::Graphql, "/query.legacypets");
    assert_eq!(gql.framework, "cdk-appsync");
    assert_eq!(handler_name(&idx, gql), "lambda_handler");
}

#[test]
fn detects_variable_tracked_resources() {
    let idx = index();
    let ep = &idx.graph.endpoints;

    // Variable-held resources: assigned_to tracking rebuilds the tree.
    let widgets = find(&idx, HttpMethod::Get, "/widgets", "varres.ts");
    assert_eq!(widgets.framework, "cdk");
    assert_eq!(widgets.confidence, Confidence::High);
    find(&idx, HttpMethod::Patch, "/widgets/{*}", "varres.ts");
    // Root method with other resources present (my-widget-service shape).
    find(&idx, HttpMethod::Head, "/", "varres.ts");
    // Chained addResource().addMethod() (the-dynamo-streamer shape).
    let orders = find(&idx, HttpMethod::Post, "/orders", "varres.ts");
    assert_eq!(orders.confidence, Confidence::High);

    // proxy:false LambdaRestApi: explicit routes on the API variable
    // suppress the proxy-all row; the explicit route carries the truth.
    find(&idx, HttpMethod::Get, "/vhello", "varres.ts");
    assert!(
        !ep.endpoints.iter().any(|e| {
            e.path_raw == "/{proxy+}"
                && idx.graph.files[e.file_id as usize].path.ends_with("varres.ts")
        }),
        "proxy:false LambdaRestApi must not emit a proxy-all row"
    );

    // StepFunctionsRestApi -> ANY on the API root.
    let sfn = find(&idx, HttpMethod::Any, "/", "varres.ts");
    assert_eq!(sfn.confidence, Confidence::Heuristic);
    // HttpApi with defaultIntegration -> the $default catch-all.
    let dflt = find(&idx, HttpMethod::Any, "/{*}", "varres.ts");
    assert_eq!(dflt.path_raw, "$default");

    // WebSocket routes: L2 ctor options, addRoute, and L1 CfnRoute keys —
    // operation names kept literal in the norm.
    assert_eq!(
        find(&idx, HttpMethod::Any, "/$connect", "varres.ts").framework,
        "cdk-websocket"
    );
    find(&idx, HttpMethod::Any, "/sendmessage", "varres.ts");
    find(&idx, HttpMethod::Any, "/$disconnect", "varres.ts");

    // HttpRouteKey.with('/v2books', HttpMethod.PUT) L2 route key.
    let books = find(&idx, HttpMethod::Put, "/v2books", "varres.ts");
    assert_eq!(books.confidence, Confidence::High);
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
