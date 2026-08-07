//! RPC-style operation detection (SOAP / XML-RPC / JSON-RPC / gRPC) and
//! name-based client correlation over the legacy-protocol fixture tree.

use gigagraph::endpoints::{ApiKind, Endpoint, EndpointIndex};
use gigagraph::indexer::build_index;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rpc");
    build_index(&root, true).expect("index build failed")
}

fn find<'a>(ep: &'a EndpointIndex, kind: ApiKind, op_norm: &str) -> &'a Endpoint {
    ep.endpoints
        .iter()
        .find(|e| e.kind == kind && e.path_norm == op_norm)
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| format!("{:?} {} ({})", e.kind, e.path_norm, e.framework))
                .collect();
            panic!("no {kind:?} op {op_norm}; got: {all:#?}")
        })
}

#[test]
fn detects_soap_operations() {
    let index = index();
    let g = &index.graph;
    let ep = &g.endpoints;

    // JAX-WS: @WebMethod, handler = the annotated method.
    let add = find(ep, ApiKind::Soap, "/add");
    assert_eq!(add.framework, "jaxws");
    let h = add.handler.expect("jaxws handler");
    assert_eq!(g.functions[h as usize].name, "add");
    // operationName override beats the method name.
    find(ep, ApiKind::Soap, "/subtractnumbers");
    // Un-annotated methods are not operations.
    assert!(!ep.endpoints.iter().any(|e| e.path_norm == "/helper"));

    // WCF [OperationContract] + classic ASMX [WebMethod].
    assert_eq!(find(ep, ApiKind::Soap, "/getorder").framework, "wcf");
    assert!(!ep.endpoints.iter().any(|e| e.path_norm == "/notexposed"));
    assert_eq!(find(ep, ApiKind::Soap, "/getquoteasmx").framework, "asmx");

    // spyne @rpc.
    let gu = find(ep, ApiKind::Soap, "/get_user");
    assert_eq!(gu.framework, "spyne");
    find(ep, ApiKind::Soap, "/list_users");

    // PHP SoapServer: addFunction (handler resolved by name) + setClass
    // (every public method), magic methods excluded by the `__` filter.
    let quote = find(ep, ApiKind::Soap, "/getquote");
    assert_eq!(quote.framework, "soap-php");
    let h = quote.handler.expect("addFunction handler");
    assert_eq!(g.functions[h as usize].name, "getQuote");
    let ping = find(ep, ApiKind::Soap, "/ping");
    assert_eq!(g.functions[ping.handler.unwrap() as usize].name, "ping");
}

#[test]
fn detects_xmlrpc_and_jsonrpc() {
    let index = index();
    let g = &index.graph;
    let ep = &g.endpoints;

    // Python SimpleXMLRPCServer.register_function.
    let mul = find(ep, ApiKind::XmlRpc, "/multiply");
    assert_eq!(mul.framework, "xmlrpc-server");
    assert_eq!(g.functions[mul.handler.unwrap() as usize].name, "multiply");

    // PHP xmlrpc_server_register_method (built-in: no import gate needed).
    let addn = find(ep, ApiKind::XmlRpc, "/addnumbers");
    assert_eq!(addn.framework, "xmlrpc-php");
    assert_eq!(
        g.functions[addn.handler.unwrap() as usize].name,
        "addNumbers"
    );

    // json-rpc-2.0 addMethod: string op, named handler resolved; inline
    // arrow handlers register the op with no handler.
    let echo = find(ep, ApiKind::JsonRpc, "/echomessage");
    assert_eq!(
        g.functions[echo.handler.unwrap() as usize].name,
        "echoMessage"
    );
    assert!(find(ep, ApiKind::JsonRpc, "/sumnumbers").handler.is_none());
}

#[test]
fn detects_grpc_services() {
    let index = index();
    let ep = &index.graph.endpoints;

    // Go RegisterGreeterServer + Python add_GreeterServicer_to_server, both
    // service-level.
    let greeters: Vec<&Endpoint> = ep
        .endpoints
        .iter()
        .filter(|e| e.kind == ApiKind::Grpc && e.path_norm == "/greeter")
        .collect();
    assert!(
        greeters.len() >= 2,
        "expected Go + Python gRPC registrations, got {greeters:?}"
    );
}

#[test]
fn correlates_rpc_clients_to_operations() {
    let index = index();
    let g = &index.graph;
    let ep = &g.endpoints;

    let matched: Vec<(&str, &str, ApiKind)> = ep
        .matches
        .iter()
        .map(|(cid, eid, _)| {
            let c = &ep.client_calls[*cid as usize];
            let e = &ep.endpoints[*eid as usize];
            (c.library.as_str(), e.path_norm.as_str(), e.kind)
        })
        .collect();

    // zeep dynamic proxy -> spyne op, by operation name.
    assert!(matched.contains(&("zeep", "/get_user", ApiKind::Soap)));
    // PHP __soapCall -> SoapServer addFunction op.
    assert!(matched.contains(&("soap-php", "/getquote", ApiKind::Soap)));
    // xmlrpc ServerProxy -> register_function op.
    assert!(matched.contains(&("xmlrpc-client", "/multiply", ApiKind::XmlRpc)));
    // JSON-RPC client.request -> addMethod op.
    assert!(matched.contains(&("json-rpc", "/echomessage", ApiKind::JsonRpc)));
    // gRPC stubs (Go NewGreeterClient, Python GreeterStub) -> service rows.
    assert!(
        matched
            .iter()
            .any(|(l, p, k)| *l == "grpc" && *p == "/greeter" && *k == ApiKind::Grpc)
    );

    // Cross-protocol leakage guard: no RPC client may match an HTTP route.
    for (cid, eid, _) in &ep.matches {
        let c = &ep.client_calls[*cid as usize];
        let e = &ep.endpoints[*eid as usize];
        assert_eq!(c.kind, e.kind, "kind-mixed match: {c:?} vs {e:?}");
    }
}
