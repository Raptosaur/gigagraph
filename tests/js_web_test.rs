//! JS/TS web-framework endpoint detection over tests/fixtures/jsweb —
//! shapes distilled from real apps (node-express-boilerplate, fastify/demo,
//! koajs/examples, nestjs-realworld-example-app) during red-green validation.

use gigagraph::endpoints::{Endpoint, HttpMethod};
use gigagraph::indexer::build_index;
use gigagraph::types::Confidence;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jsweb");
    build_index(&root, true).expect("index build failed")
}

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

fn handler_name<'a>(idx: &'a gigagraph::indexer::Index, e: &Endpoint) -> &'a str {
    let h = e.handler.unwrap_or_else(|| {
        panic!(
            "endpoint {} {} has no handler",
            e.method.as_str(),
            e.path_norm
        )
    });
    &idx.graph.functions[h as usize].name
}

#[test]
fn express_route_chains_and_transitive_mounts() {
    let idx = index();

    // router.route('/').get(...).post(...): the route() call defines the
    // path, the chained verbs (receiver-less, same chain-start byte) bind
    // the methods. app.js mounts api.js at /api/v1, api.js mounts
    // users.route.js at /users — the composed prefix reaches the endpoints.
    let list = find(&idx, HttpMethod::Get, "/api/v1/users", "users.route.js");
    assert_eq!(list.framework, "express");
    assert_eq!(list.confidence, Confidence::Heuristic); // mount join
    let create = find(&idx, HttpMethod::Post, "/api/v1/users", "users.route.js");
    let remove = find(
        &idx,
        HttpMethod::Delete,
        "/api/v1/users/{*}",
        "users.route.js",
    );

    // Dotted controller handlers (`userController.list`) resolve through
    // the require import to users.controller.js; the handler ident is the
    // LAST argument — middleware (`auth(...)`) must not win.
    assert_eq!(handler_name(&idx, list), "list");
    assert_eq!(handler_name(&idx, create), "create");
    assert_eq!(handler_name(&idx, remove), "remove");
}

#[test]
fn koa_router_prefix_and_factory_import() {
    let idx = index();

    // new Router({ prefix: '/kapi' }): ctor options + assigned_to binding.
    let pets = find(&idx, HttpMethod::Get, "/kapi/pets", "koa/app.js");
    assert_eq!(pets.framework, "koa");
    assert_eq!(handler_name(&idx, pets), "listPets");

    // require('@koa/router')() factory: the import still registers as
    // evidence; verb-on-verb chains keep their own path literals.
    let show = find(&idx, HttpMethod::Get, "/factory-route", "factory.js");
    assert_eq!(show.framework, "koa");
    assert_eq!(show.confidence, Confidence::High);
    assert_eq!(handler_name(&idx, show), "show");
    let post = find(&idx, HttpMethod::Post, "/factory-post", "factory.js");
    assert_eq!(handler_name(&idx, post), "createIt");
}

#[test]
fn fastify_register_prefix_and_autoload() {
    let idx = index();

    // fastify.register(userRoutes, { prefix: '/v2' }): cross-file mount.
    let profile = find(&idx, HttpMethod::Get, "/v2/profile", "users.routes.js");
    assert_eq!(profile.framework, "fastify");
    assert_eq!(profile.confidence, Confidence::Heuristic);
    // Options-object route form uses `url`, not `path`.
    find(&idx, HttpMethod::Put, "/v2/settings", "users.routes.js");

    // @fastify/autoload (package.json evidence): routes/tasks/index.js
    // registers under the directory-derived /tasks prefix.
    let tasks = find(&idx, HttpMethod::Get, "/tasks", "routes/tasks/index.js");
    assert_eq!(tasks.confidence, Confidence::Heuristic);
    find(&idx, HttpMethod::Post, "/tasks/run", "routes/tasks/index.js");
}

#[test]
fn nest_global_prefix() {
    let idx = index();

    // app.setGlobalPrefix('gapi') in main.ts joins every controller route;
    // cross-file assumption -> Heuristic.
    let one = find(&idx, HttpMethod::Get, "/gapi/cats/{*}", "cats.controller.ts");
    assert_eq!(one.framework, "nestjs");
    assert_eq!(one.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, one), "findOne");
    let create = find(&idx, HttpMethod::Post, "/gapi/cats", "cats.controller.ts");
    assert_eq!(handler_name(&idx, create), "create");
}
