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

#[test]
fn fastify_multi_method_route_array() {
    let idx = index();

    // route({ method: ['GET', 'POST'], url: '/bulk' }): the multi-member
    // method array arrives `method`-keyed (harvester depth-3 array reach)
    // -> one row per verb, composed under the /v2 register prefix.
    let get = find(&idx, HttpMethod::Get, "/v2/bulk", "users.routes.js");
    assert_eq!(get.framework, "fastify");
    assert_eq!(get.confidence, Confidence::Heuristic);
    find(&idx, HttpMethod::Post, "/v2/bulk", "users.routes.js");
    assert!(
        !idx.graph
            .endpoints
            .endpoints
            .iter()
            .any(|e| e.method == HttpMethod::Any && e.path_norm == "/v2/bulk"),
        "multi-method array must not degrade to ANY"
    );
}

#[test]
fn nest_multi_path_controller_and_uri_version() {
    let idx = index();

    // @Controller(['bulk', 'batch']): one route set per prefix.
    let jobs = find(&idx, HttpMethod::Get, "/gapi/bulk/jobs", "dual.controller.ts");
    assert_eq!(jobs.framework, "nestjs");
    assert_eq!(handler_name(&idx, jobs), "jobs");
    find(&idx, HttpMethod::Get, "/gapi/batch/jobs", "dual.controller.ts");

    // @Version('1') + enableVersioning({type: URI}) in main.ts: the /v1
    // segment joins right after the global prefix, on BOTH controller
    // prefixes; cross-file assumption -> Heuristic.
    let status = find(
        &idx,
        HttpMethod::Get,
        "/gapi/v1/bulk/status",
        "dual.controller.ts",
    );
    assert_eq!(status.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, status), "status");
    find(
        &idx,
        HttpMethod::Get,
        "/gapi/v1/batch/status",
        "dual.controller.ts",
    );

    // Undecorated methods must NOT pick up the version prefix (and the
    // versioned method must not keep an unversioned row).
    let ep = &idx.graph.endpoints;
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.path_norm == "/gapi/v1/bulk/jobs" || e.path_norm == "/gapi/bulk/status"),
        "version prefix must apply to exactly the @Version'd methods"
    );
}

#[test]
fn nest_router_module_prefixes() {
    let idx = index();

    // RouterModule.register([{path: 'admin', module: AdminModule}]) in
    // app.module.ts: AdminModule resolves through the import to
    // admin.module.ts, whose @Module({controllers: [AdminController,
    // AuditController]}) maps both controllers under /admin. Name-keyed
    // cross-file joins -> Heuristic.
    let stats = find(
        &idx,
        HttpMethod::Get,
        "/gapi/admin/dash/stats",
        "admin.controller.ts",
    );
    assert_eq!(stats.framework, "nestjs");
    assert_eq!(stats.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&idx, stats), "stats");
    let log = find(
        &idx,
        HttpMethod::Get,
        "/gapi/admin/audit/log",
        "audit.controller.ts",
    );
    assert_eq!(handler_name(&idx, log), "log");

    // Controllers registered directly on AppModule (cats) keep their
    // module-prefix-free paths — nest_global_prefix asserts those rows.
    let ep = &idx.graph.endpoints;
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.path_norm.starts_with("/gapi/admin/cats")),
        "unregistered controllers must not inherit the module prefix"
    );
}
