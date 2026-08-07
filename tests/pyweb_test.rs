//! Python web-framework endpoint detection over the pyweb fixture tree —
//! distilled from red-green validation against real apps: miguelgrinberg/
//! microblog (Flask blueprints), fastapi/full-stack-fastapi-template
//! (APIRouter include chains), HackSoftware/Django-Styleguide-Example
//! (include() chains + inline lists + CBVs), TandoorRecipes/recipes (DRF
//! routers, @action mixins), aio-libs/aiohttp-demos (router registration).

use gigagraph::endpoints::{Endpoint, EndpointIndex, HttpMethod};
use gigagraph::indexer::build_index;
use gigagraph::types::Confidence;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pyweb");
    build_index(&root, true).expect("index build failed")
}

fn find<'a>(ep: &'a EndpointIndex, fw: &str, method: HttpMethod, norm: &str) -> &'a Endpoint {
    ep.endpoints
        .iter()
        .find(|e| e.framework == fw && e.method == method && e.path_norm == norm)
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| format!("{} {} ({})", e.method.as_str(), e.path_norm, e.framework))
                .collect();
            panic!("no {fw} endpoint {} {norm}; got: {all:#?}", method.as_str())
        })
}

fn handler_name(index: &gigagraph::indexer::Index, e: &Endpoint) -> String {
    index.graph.functions[e.handler.expect("handler resolved") as usize]
        .name
        .clone()
}

#[test]
fn flask_blueprint_prefixes_compose_cross_file() {
    let index = index();
    let ep = &index.graph.endpoints;

    // register_blueprint(auth_bp, url_prefix='/auth') joins routes declared
    // in a different file than both the registration and the Blueprint().
    let login = find(ep, "flask", HttpMethod::Post, "/auth/login");
    assert_eq!(login.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&index, login), "login");
    find(ep, "flask", HttpMethod::Get, "/auth/login");
    // Flask 2 verb shorthand @bp.get.
    find(ep, "flask", HttpMethod::Get, "/auth/logout");

    // Blueprint's own url_prefix ('/v1') applies when the registration has
    // no kwarg — and the route module imports NOTHING from flask (framework
    // evidence travels through the blueprint binding).
    let create = find(ep, "flask", HttpMethod::Post, "/v1/tokens");
    assert_eq!(handler_name(&index, create), "create_token");
    find(ep, "flask", HttpMethod::Delete, "/v1/tokens");

    // Unprefixed blueprint: registration known -> cross-file Heuristic.
    let root = find(ep, "flask", HttpMethod::Get, "/");
    assert_eq!(handler_name(&index, root), "index");

    // Route converters survive extraction intact (strip_quotes must not eat
    // one-sided angle brackets).
    let profile = find(ep, "flask", HttpMethod::Get, "/user/{*}");
    assert_eq!(profile.path_raw, "/user/<username>");

    // MethodView via add_url_rule: verbs from the class's methods.
    let counter_get = find(ep, "flask", HttpMethod::Get, "/counter");
    assert_eq!(handler_name(&index, counter_get), "get");
    let counter_post = find(ep, "flask", HttpMethod::Post, "/counter");
    assert_eq!(handler_name(&index, counter_post), "post");
}

#[test]
fn fastapi_router_chains_compose_transitively() {
    let index = index();
    let ep = &index.graph.endpoints;

    // app.include_router(api_router, prefix=settings.API_V1_STR) — the
    // prefix Ident resolves through the settings import to a class-level
    // string constant — then api_router.include_router(items.router) joins
    // APIRouter(prefix="/items").
    let items = find(ep, "fastapi", HttpMethod::Get, "/api/v1/items");
    assert_eq!(items.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&index, items), "read_items");
    find(ep, "fastapi", HttpMethod::Put, "/api/v1/items/{*}");
    find(ep, "fastapi", HttpMethod::Delete, "/api/v1/items/{*}");

    // Router with no own prefix: chain prefix + full literal path.
    let login = find(ep, "fastapi", HttpMethod::Post, "/api/v1/login/access-token");
    assert_eq!(handler_name(&index, login), "login_access_token");
}

#[test]
fn django_include_chains_and_class_views() {
    let index = index();
    let ep = &index.graph.endpoints;

    // Unmounted root row keeps High confidence.
    let admin = find(ep, "django", HttpMethod::Any, "/admin");
    assert_eq!(admin.confidence, Confidence::High);

    // Two-level include chain + APIView verb expansion: config 'api/' ->
    // api 'users/' -> path("", UserListApi.as_view()).
    let list = find(ep, "django", HttpMethod::Get, "/api/users");
    assert_eq!(list.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&index, list), "get");
    find(ep, "django", HttpMethod::Post, "/api/users");
    let detail = find(ep, "django", HttpMethod::Get, "/api/users/{*}");
    assert_eq!(detail.path_raw, "/api/users/<int:user_id>/");

    // Legacy url() row joins the mount chain, stays Heuristic.
    let legacy = find(ep, "django", HttpMethod::Any, "/api/users/export");
    assert_eq!(legacy.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&index, legacy), "export_users");

    // Inline include(([path(...)], 'ns')) nesting joins by byte containment.
    find(ep, "django", HttpMethod::Get, "/api/upload/start");

    // include()-bearing path() rows are mounts, not routes.
    for mount in ["/api", "/drf", "/drf/v1", "/api/users/"] {
        assert!(
            !ep.endpoints.iter().any(|e| e.path_norm == *mount),
            "mount row {mount} must be suppressed"
        );
    }
}

#[test]
fn drf_router_register_expands_by_viewset_shape() {
    let index = index();
    let ep = &index.graph.endpoints;

    // ModelViewSet: full conventional set under the composed prefix
    // (config 'drf/' -> urls 'v1/' -> register('articles', ...)).
    let list = find(ep, "drf", HttpMethod::Get, "/drf/v1/articles");
    assert_eq!(list.confidence, Confidence::Heuristic);
    assert_eq!(handler_name(&index, list), "list");
    find(ep, "drf", HttpMethod::Post, "/drf/v1/articles");
    find(ep, "drf", HttpMethod::Get, "/drf/v1/articles/{*}");
    find(ep, "drf", HttpMethod::Put, "/drf/v1/articles/{*}");
    find(ep, "drf", HttpMethod::Patch, "/drf/v1/articles/{*}");
    find(ep, "drf", HttpMethod::Delete, "/drf/v1/articles/{*}");

    // @action(detail=True) -> /<pk>/publish/; boolean kwarg is harvested.
    let publish = find(ep, "drf", HttpMethod::Post, "/drf/v1/articles/{*}/publish");
    assert_eq!(handler_name(&index, publish), "publish");
    // @action(detail=False, url_path='export/(?P<fmt>[^/.]+)'): regex
    // segment folds to one parameter.
    let export = find(ep, "drf", HttpMethod::Get, "/drf/v1/articles/export/{*}");
    assert_eq!(handler_name(&index, export), "export");

    // ReadOnlyModelViewSet: list + retrieve only.
    find(ep, "drf", HttpMethod::Get, "/drf/v1/feeds");
    find(ep, "drf", HttpMethod::Get, "/drf/v1/feeds/{*}");
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.path_norm.starts_with("/drf/v1/feeds") && e.method != HttpMethod::Get),
        "read-only viewset must not expand write routes"
    );

    // GenericViewSet with no standard action methods: only its @action.
    find(ep, "drf", HttpMethod::Get, "/drf/v1/stats/summary");
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.path_norm == "/drf/v1/stats" || e.path_norm == "/drf/v1/stats/{*}"),
        "GenericViewSet without action methods binds no conventional routes"
    );
}

#[test]
fn aiohttp_router_registrations() {
    let index = index();
    let ep = &index.graph.endpoints;

    let root = find(ep, "aiohttp", HttpMethod::Get, "/");
    assert_eq!(root.confidence, Confidence::High);
    assert_eq!(handler_name(&index, root), "index");
    find(ep, "aiohttp", HttpMethod::Post, "/vote/{*}");

    // web.get(...) rows inside app.add_routes([...]).
    let ws = find(ep, "aiohttp", HttpMethod::Get, "/ws");
    assert_eq!(handler_name(&index, ws), "ws_handler");

    // Aliased bare add_route('GET', '/graphql', gql).
    let gql = find(ep, "aiohttp", HttpMethod::Get, "/graphql");
    assert_eq!(handler_name(&index, gql), "gql");

    // RouteTableDef decorator.
    let table = find(ep, "aiohttp", HttpMethod::Get, "/table");
    assert_eq!(handler_name(&index, table), "table");

    // Route module with no aiohttp import: admitted by the project
    // dependency manifest, at Heuristic.
    let items = find(ep, "aiohttp", HttpMethod::Get, "/items");
    assert_eq!(items.confidence, Confidence::Heuristic);
    find(ep, "aiohttp", HttpMethod::Post, "/items");

    assert!(
        !ep.endpoints.iter().any(|e| e.path_norm.contains("static")),
        "add_static is not a route"
    );
}
