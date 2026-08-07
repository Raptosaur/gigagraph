//! Go web-framework endpoint detection over the goweb fixture tree:
//! group-prefix composition through variables (gin/echo/fiber), chi
//! Route-closure nesting and cross-file Mounts, gorilla subrouter chains,
//! and cross-file register functions — distilled from red/green validation
//! against real apps (gothinkster realworld gin, dhax/go-base, chi
//! _examples, gofiber/recipes, echox cookbook, podinfo,
//! go-todo-rest-api-example).

use gigagraph::endpoints::{Endpoint, EndpointIndex, HttpMethod};
use gigagraph::indexer::build_index;
use gigagraph::types::Confidence;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/goweb");
    build_index(&root, true).expect("index build failed")
}

fn find<'a>(ep: &'a EndpointIndex, method: HttpMethod, raw: &str) -> &'a Endpoint {
    ep.endpoints
        .iter()
        .find(|e| e.method == method && e.path_raw == raw)
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| format!("{} {} ({})", e.method.as_str(), e.path_raw, e.framework))
                .collect();
            panic!("no endpoint {} {raw}; got: {all:#?}", method.as_str())
        })
}

#[test]
fn gin_group_variables_and_cross_file_registers() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // `v1 := r.Group("/api")` + `users.UsersRegister(v1.Group("/users"))`:
    // the register function's routes (declared in ANOTHER file, with an
    // empty-string path) compose the full prefix.
    let reg = find(ep, HttpMethod::Post, "/api/users");
    assert_eq!(reg.framework, "gin");
    assert_eq!(reg.confidence, Confidence::High);
    assert_eq!(
        g.functions[reg.handler.unwrap() as usize].name,
        "UsersRegistration"
    );
    find(ep, HttpMethod::Post, "/api/users/login");

    // Nested group variable: `admin := v1.Group("/admin")`.
    find(ep, HttpMethod::Get, "/api/admin/stats");

    // Group bound straight off the engine; "/" collapses into the prefix.
    find(ep, HttpMethod::Get, "/api/ping");

    // No unprefixed leftovers of the composed routes.
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.framework == "gin" && (e.path_raw == "/login" || e.path_raw == "/stats"))
    );
}

#[test]
fn chi_route_nesting_and_transitive_mounts() {
    let index = index();
    let ep = &index.graph.endpoints;

    // Route-closure nesting by byte containment.
    let list = find(ep, HttpMethod::Get, "/articles");
    assert_eq!(list.framework, "chi");
    assert_eq!(list.confidence, Confidence::High);
    find(ep, HttpMethod::Post, "/articles");
    find(ep, HttpMethod::Get, "/articles/{articleID}");

    // Cross-file Mount (`r.Mount("/admin", admin.Router())` inside a Group
    // closure) — unique target name resolves High.
    let admin = find(ep, HttpMethod::Get, "/admin");
    assert_eq!(admin.confidence, Confidence::High);

    // Second-level Mount inside the mounted router composes transitively;
    // its composite-literal target (`accountsResource{}.routes()`) resolves
    // through the receiver~type fuzz, capping confidence at Heuristic.
    let accounts = find(ep, HttpMethod::Get, "/admin/accounts");
    assert_eq!(accounts.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Put, "/admin/accounts/{accountID}");
    find(ep, HttpMethod::Get, "/admin/groups");

    // Un-prefixed /healthz registered on the bare router stays as-is.
    find(ep, HttpMethod::Get, "/healthz");
}

#[test]
fn fiber_groups_including_chained_use() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // Nested group variables.
    let users = find(ep, HttpMethod::Get, "/api/v1/users");
    assert_eq!(users.framework, "fiber");
    assert_eq!(users.confidence, Confidence::High);
    assert_eq!(
        g.functions[users.handler.unwrap() as usize].name,
        "listUsers"
    );

    // Slash-less Group path ("ping") is normalized into the prefix.
    find(ep, HttpMethod::Get, "/ping/pong");

    // `todo := app.Group("/todo").Use(auth)` — the segment lives on the
    // inner Group call of the chain.
    find(ep, HttpMethod::Get, "/todo/list");
    find(ep, HttpMethod::Post, "/todo/create");
}

#[test]
fn echo_groups_with_empty_paths() {
    let index = index();
    let ep = &index.graph.endpoints;

    // `g := e.Group("/manage"); g.GET("", h)` — empty path IS the group root.
    let home = find(ep, HttpMethod::Get, "/manage");
    assert_eq!(home.framework, "echo");

    // Nested group.
    find(ep, HttpMethod::Get, "/manage/users/:id");
}

#[test]
fn gorilla_subrouter_chains_and_wrapper_verbs() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // `api := r.PathPrefix("/api/v4").Subrouter()`; the path-builder ident
    // (`prefixedPath`) shares the path's arg index and is skipped, so the
    // handler resolves to the same-package sibling-file function.
    let version = find(ep, HttpMethod::Get, "/api/v4/version");
    assert_eq!(version.framework, "gorilla");
    assert_eq!(
        g.functions[version.handler.unwrap() as usize].name,
        "versionHandler"
    );

    // Struct-field subrouter (`b.Users = api.PathPrefix("/users")
    // .Subrouter()`) composes nested prefixes; "" registers the group root.
    find(ep, HttpMethod::Get, "/api/v4/users");
    find(ep, HttpMethod::Get, "/api/v4/users/{id}");
    find(ep, HttpMethod::Delete, "/api/v4/users/{id}");

    // Wrapper-verb convention (`a.Get("/projects", a.handleRequest(...))`):
    // detected as Heuristic, handler resolved via global uniqueness.
    let projects = find(ep, HttpMethod::Get, "/projects");
    assert_eq!(projects.framework, "gorilla");
    assert_eq!(projects.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[projects.handler.unwrap() as usize].name,
        "GetAllProjects"
    );
    find(ep, HttpMethod::Post, "/projects");
    find(ep, HttpMethod::Put, "/projects/{title}");
}
