//! End-to-end endpoint detection + client correlation over the polyglot
//! fixture tree.

use gigagraph::endpoints::{Endpoint, EndpointIndex, HttpMethod, normalize_path};
use gigagraph::indexer::build_index;
use std::path::Path;

fn index() -> gigagraph::indexer::Index {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/endpoints");
    build_index(&root, true).expect("index build failed")
}

fn find<'a>(ep: &'a EndpointIndex, method: HttpMethod, norm: &str) -> &'a Endpoint {
    ep.endpoints
        .iter()
        .find(|e| e.method == method && e.path_norm == norm)
        .unwrap_or_else(|| {
            let all: Vec<String> = ep
                .endpoints
                .iter()
                .map(|e| format!("{} {} ({})", e.method.as_str(), e.path_norm, e.framework))
                .collect();
            panic!("no endpoint {} {norm}; got: {all:#?}", method.as_str())
        })
}

#[test]
fn normalizes_paths() {
    assert_eq!(normalize_path("/users/:id").as_deref(), Some("/users/{*}"));
    assert_eq!(normalize_path("/users/{id}").as_deref(), Some("/users/{*}"));
    assert_eq!(
        normalize_path("/items/<int:item_id>").as_deref(),
        Some("/items/{*}")
    );
    assert_eq!(
        normalize_path("https://api.example.com/metrics?x=1").as_deref(),
        Some("/metrics")
    );
    assert_eq!(
        normalize_path("/Users/${id}/Posts").as_deref(),
        Some("/users/{*}/posts")
    );
}

#[test]
fn detects_endpoints_across_frameworks() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // Express: named handler resolved.
    let e = find(ep, HttpMethod::Get, "/users/{*}");
    assert_eq!(e.framework, "express");
    let handler = e.handler.expect("express handler resolved");
    assert_eq!(g.functions[handler as usize].name, "getUser");
    find(ep, HttpMethod::Post, "/users");

    // Flask: methods kwarg + converter segment; handler is the decorated fn.
    let create = find(ep, HttpMethod::Post, "/items");
    assert_eq!(create.framework, "flask");
    assert_eq!(
        g.functions[create.handler.unwrap() as usize].name,
        "create_item"
    );
    find(ep, HttpMethod::Get, "/items/{*}");

    // Laravel Route:: calls.
    find(ep, HttpMethod::Get, "/orders/{*}");
    find(ep, HttpMethod::Post, "/orders");

    // Symfony attribute: two methods from `methods: ['GET', 'PUT']`.
    let show = find(ep, HttpMethod::Get, "/profiles/{*}");
    assert_eq!(show.framework, "symfony");
    find(ep, HttpMethod::Put, "/profiles/{*}");

    // Spring annotations, incl. method from RequestMethod.POST field access.
    find(ep, HttpMethod::Get, "/accounts/{*}");
    let spring_post = find(ep, HttpMethod::Post, "/accounts");
    assert_eq!(spring_post.framework, "spring");

    // Go 1.22 method-in-pattern + plain HandleFunc -> ANY.
    let health = find(ep, HttpMethod::Get, "/health");
    assert_eq!(health.framework, "net/http");
    assert_eq!(g.functions[health.handler.unwrap() as usize].name, "health");
    find(ep, HttpMethod::Any, "/webhook");

    // Sinatra.
    find(ep, HttpMethod::Get, "/ping");
    find(ep, HttpMethod::Post, "/echo");

    // ASP.NET attribute routing.
    let cs = find(ep, HttpMethod::Get, "/api/users/{*}");
    assert_eq!(cs.framework, "aspnet");

    // axum: method + handler from the nested get(...) call.
    let ax = find(ep, HttpMethod::Get, "/widgets/{*}");
    assert_eq!(ax.framework, "axum");
    assert_eq!(
        g.functions[ax.handler.unwrap() as usize].name,
        "show_widget"
    );
}

#[test]
fn detects_phase2_server_frameworks() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // NestJS: @Controller("gadgets") class prefix joined onto method decos.
    let nest = find(ep, HttpMethod::Get, "/gadgets/{*}");
    assert_eq!(nest.framework, "nestjs");
    assert_eq!(nest.confidence, Confidence::High);
    assert_eq!(
        g.functions[nest.handler.unwrap() as usize].name,
        "getGadget"
    );
    let nest_post = find(ep, HttpMethod::Post, "/gadgets");
    assert_eq!(
        g.functions[nest_post.handler.unwrap() as usize].name,
        "createGadget"
    );

    // Django urls.py: path() High, legacy url() Heuristic, regex re_path skipped.
    let dj = find(ep, HttpMethod::Any, "/reports/{*}");
    assert_eq!(dj.framework, "django");
    assert_eq!(dj.confidence, Confidence::High);
    let dj_index = find(ep, HttpMethod::Any, "/reports");
    assert_eq!(
        g.functions[dj_index.handler.unwrap() as usize].name,
        "report_index"
    );
    let dj_legacy = find(ep, HttpMethod::Any, "/archive");
    assert_eq!(dj_legacy.confidence, Confidence::Heuristic);
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !e.path_norm.contains("articles")),
        "regex re_path must be skipped"
    );

    // Rails resources/resource expansion (Heuristic) + explicit verb (High).
    for (m, p) in [
        (HttpMethod::Get, "/posts"),
        (HttpMethod::Get, "/posts/new"),
        (HttpMethod::Post, "/posts"),
        (HttpMethod::Get, "/posts/{*}"),
        (HttpMethod::Get, "/posts/{*}/edit"),
        (HttpMethod::Patch, "/posts/{*}"),
        (HttpMethod::Delete, "/posts/{*}"),
        (HttpMethod::Get, "/account"),
        (HttpMethod::Get, "/account/new"),
        (HttpMethod::Get, "/account/edit"),
        (HttpMethod::Post, "/account"),
        (HttpMethod::Patch, "/account"),
        (HttpMethod::Delete, "/account"),
    ] {
        let e = find(ep, m, p);
        assert_eq!(e.framework, "rails");
        assert_eq!(e.confidence, Confidence::Heuristic);
    }
    let dash = find(ep, HttpMethod::Get, "/dashboard");
    assert_eq!(dash.framework, "rails");
    assert_eq!(dash.confidence, Confidence::High);

    // Laravel: match array, any, resource expansion.
    assert_eq!(find(ep, HttpMethod::Get, "/mixed").framework, "laravel");
    find(ep, HttpMethod::Post, "/mixed");
    find(ep, HttpMethod::Any, "/anything");
    for (m, p) in [
        (HttpMethod::Get, "/photos"),
        (HttpMethod::Get, "/photos/new"),
        (HttpMethod::Post, "/photos"),
        (HttpMethod::Get, "/photos/{*}"),
        (HttpMethod::Get, "/photos/{*}/edit"),
        (HttpMethod::Patch, "/photos/{*}"),
        (HttpMethod::Delete, "/photos/{*}"),
    ] {
        let e = find(ep, m, p);
        assert_eq!(e.framework, "laravel");
        assert_eq!(e.confidence, Confidence::Heuristic);
    }

    // Slim: $app->get with Slim import evidence.
    let slim = find(ep, HttpMethod::Get, "/brews");
    assert_eq!(slim.framework, "slim");
    find(ep, HttpMethod::Post, "/brews");

    // gorilla/mux: chained .Methods("GET").
    let mux = find(ep, HttpMethod::Get, "/tasks/{*}");
    assert_eq!(mux.framework, "gorilla");
    assert_eq!(g.functions[mux.handler.unwrap() as usize].name, "taskShow");
    assert_eq!(find(ep, HttpMethod::Post, "/tasks").framework, "gorilla");

    // actix attribute macros.
    let actix = find(ep, HttpMethod::Get, "/invoices/{*}");
    assert_eq!(actix.framework, "actix");
    assert_eq!(
        g.functions[actix.handler.unwrap() as usize].name,
        "show_invoice"
    );
    find(ep, HttpMethod::Post, "/invoices");

    // Ktor bare verb calls (Heuristic).
    let ktor = find(ep, HttpMethod::Get, "/telemetry/live");
    assert_eq!(ktor.framework, "ktor");
    assert_eq!(ktor.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Post, "/telemetry");

    // Spring-Kotlin method annotation.
    assert_eq!(
        find(ep, HttpMethod::Get, "/ledgers/{*}").framework,
        "spring"
    );

    // Accurate labels for koa-router / hono / restify.
    let koa = find(ep, HttpMethod::Get, "/koalas/{*}");
    assert_eq!(koa.framework, "koa");
    assert_eq!(g.functions[koa.handler.unwrap() as usize].name, "koalaShow");
    assert_eq!(find(ep, HttpMethod::Get, "/hedgehogs").framework, "hono");
    let rest = find(ep, HttpMethod::Get, "/rhinos");
    assert_eq!(rest.framework, "restify");
    assert_eq!(
        g.functions[rest.handler.unwrap() as usize].name,
        "listRhinos"
    );
}

#[test]
fn detects_phase2_clients() {
    let index = index();
    let ep = &index.graph.endpoints;
    use gigagraph::types::Confidence;

    let client = |url: &str| {
        ep.client_calls
            .iter()
            .find(|c| c.url_raw == url)
            .unwrap_or_else(|| {
                let all: Vec<String> = ep
                    .client_calls
                    .iter()
                    .map(|c| format!("{} {} ({})", c.method.as_str(), c.url_raw, c.library))
                    .collect();
                panic!("no client call {url}; got {all:#?}")
            })
    };
    let match_of = |cid: u32| ep.matches.iter().find(|(c, _, _)| *c == cid);

    // XMLHttpRequest#open -> sinatra POST /echo, capped Heuristic.
    let xhr = client("/echo");
    assert_eq!(xhr.library, "xhr");
    assert_eq!(xhr.method, HttpMethod::Post);
    assert_eq!(xhr.confidence, Confidence::Heuristic);
    let m = match_of(xhr.id).expect("xhr matched");
    assert_eq!(ep.endpoints[m.1 as usize].framework, "sinatra");
    assert_eq!(m.2, Confidence::Heuristic);

    // jQuery: $.ajax (legacy `type:`), $.get, jQuery.post.
    let ajax = client("/ping");
    assert_eq!(ajax.library, "jquery");
    assert_eq!(ajax.method, HttpMethod::Get);
    let jq_get = client("/dashboard");
    assert_eq!(jq_get.library, "jquery");
    assert!(
        match_of(jq_get.id)
            .is_some_and(|(_, eid, _)| ep.endpoints[*eid as usize].framework == "rails")
    );
    let jq_post = client("/mixed");
    assert_eq!(jq_post.method, HttpMethod::Post);
    assert!(
        match_of(jq_post.id)
            .is_some_and(|(_, eid, _)| ep.endpoints[*eid as usize].framework == "laravel")
    );

    // got / ky / superagent, import-gated.
    assert_eq!(client("https://got.example.com/gizmos").library, "got");
    assert_eq!(
        client("https://got.example.com/gizmos").method,
        HttpMethod::Post
    );
    assert_eq!(client("/kites").library, "ky");
    assert_eq!(client("/sprockets").library, "superagent");

    // axios.create baseURL joined file-wide onto instance calls (Heuristic).
    let api = client("https://svc.example.com/api/tasks");
    assert_eq!(api.library, "axios");
    assert_eq!(api.confidence, Confidence::Heuristic);
    assert_eq!(api.path_norm, "/api/tasks");

    // aiohttp session receiver; correlation capped by client confidence.
    let aio = client("/items/3");
    assert_eq!(aio.library, "aiohttp");
    assert_eq!(aio.confidence, Confidence::Heuristic);
    let m = match_of(aio.id).expect("aiohttp matched");
    assert_eq!(ep.endpoints[m.1 as usize].framework, "flask");
    assert_eq!(m.2, Confidence::Heuristic);

    // Retrofit (Java): interface annotations correlate to spring, High.
    let rf = client("/accounts/{id}");
    assert_eq!(rf.library, "retrofit");
    assert_eq!(rf.method, HttpMethod::Get);
    let m = match_of(rf.id).expect("retrofit matched");
    assert_eq!(ep.endpoints[m.1 as usize].framework, "spring");
    assert_eq!(m.2, Confidence::High);
    assert_eq!(client("/accounts").method, HttpMethod::Post);

    // Retrofit (Kotlin annotation query) -> axum endpoint, High.
    let rfk = client("/widgets/{id}");
    assert_eq!(rfk.library, "retrofit");
    let m = match_of(rfk.id).expect("kotlin retrofit matched");
    assert_eq!(ep.endpoints[m.1 as usize].framework, "axum");
    assert_eq!(m.2, Confidence::High);

    // Ruby Net::HTTP with a visible URL literal.
    let nh = client("https://status.example.com/ping");
    assert_eq!(nh.library, "net-http-rb");
    assert_eq!(nh.method, HttpMethod::Get);
    assert!(
        match_of(nh.id)
            .is_some_and(|(_, eid, _)| ep.endpoints[*eid as usize].framework == "sinatra")
    );
}

#[test]
fn detects_phase3_prefix_joins() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Express cross-file mount: app.use("/shop", shopRoutes) in mounts_app.ts
    // prefixes endpoints declared in mounts_routes.ts.
    let shop = find(ep, HttpMethod::Get, "/shop/skus/{*}");
    assert_eq!(shop.framework, "express");
    assert_eq!(shop.confidence, Confidence::Heuristic);
    assert_eq!(g.functions[shop.handler.unwrap() as usize].name, "listSkus");

    // Same-file Router() mount, keyed by the endpoint call's receiver.
    let adm = find(ep, HttpMethod::Get, "/admin/settings");
    assert_eq!(adm.framework, "express");
    assert_eq!(adm.confidence, Confidence::Heuristic);
    // Direct app.get in the mounting file stays unprefixed and High.
    let status = find(ep, HttpMethod::Get, "/status");
    assert_eq!(status.confidence, Confidence::High);

    // Laravel Route::prefix()->group() containment, single and nested.
    let stats = find(ep, HttpMethod::Get, "/admin/stats");
    assert_eq!(stats.framework, "laravel");
    assert_eq!(stats.confidence, Confidence::Heuristic);
    let flags = find(ep, HttpMethod::Get, "/v2/beta/flags");
    assert_eq!(flags.confidence, Confidence::Heuristic);
    // Route::group(['prefix' => ...]) array form: the harvester digs
    // array-element initializers, so the prefix joins.
    let import = find(ep, HttpMethod::Post, "/legacy/import");
    assert_eq!(import.confidence, Confidence::Heuristic);

    // PHP define() const joined into the path (the one visible const shape).
    let carts = find(ep, HttpMethod::Get, "/shop/carts/{*}");
    assert_eq!(carts.framework, "laravel");
    assert_eq!(carts.confidence, Confidence::Heuristic);

    // Spring class-level @RequestMapping prefix via ride-along association,
    // applied to every method of the class.
    let rep = find(ep, HttpMethod::Get, "/admin/api/reports");
    assert_eq!(rep.framework, "spring");
    assert_eq!(rep.confidence, Confidence::Heuristic);
    assert_eq!(g.functions[rep.handler.unwrap() as usize].name, "reports");
    let jobs = find(ep, HttpMethod::Post, "/admin/api/jobs");
    assert_eq!(g.functions[jobs.handler.unwrap() as usize].name, "startJob");
    // The ride-along class annotation itself must not become a route.
    assert!(
        ep.endpoints.iter().all(|e| e.path_norm != "/admin/api"),
        "class-level @RequestMapping must be a prefix, not an endpoint"
    );
    // Method-level spring endpoints without a class prefix stay High.
    assert_eq!(
        find(ep, HttpMethod::Get, "/accounts/{*}").confidence,
        Confidence::High
    );

    // actix scoped attribute #[actix_web::get(...)].
    let probe = find(ep, HttpMethod::Get, "/probes/live");
    assert_eq!(probe.framework, "actix");
    assert_eq!(
        g.functions[probe.handler.unwrap() as usize].name,
        "live_probe"
    );

    // Ktor route("/api") nesting via byte containment, single and nested.
    let ku = find(ep, HttpMethod::Get, "/api/users");
    assert_eq!(ku.framework, "ktor");
    assert_eq!(ku.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Delete, "/api/v2/sessions");
    // Un-nested ktor verbs keep their bare paths.
    find(ep, HttpMethod::Get, "/telemetry/live");

    // Guzzle client URL built from a define() const; correlates to the
    // Spring class-prefixed endpoint, capped Heuristic on both sides.
    let gz = ep
        .client_calls
        .iter()
        .find(|c| c.url_raw == "/admin/api/reports")
        .expect("guzzle const-joined client call");
    assert_eq!(gz.library, "guzzle");
    assert_eq!(gz.method, HttpMethod::Get);
    assert_eq!(gz.confidence, Confidence::Heuristic);
    let m = ep
        .matches
        .iter()
        .find(|(cid, _, _)| *cid == gz.id)
        .expect("guzzle call matched");
    assert_eq!(ep.endpoints[m.1 as usize].framework, "spring");
    assert_eq!(m.2, Confidence::Heuristic);
}

#[test]
fn correlates_clients_to_endpoints() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    let client = |url: &str| {
        ep.client_calls
            .iter()
            .find(|c| c.url_raw == url)
            .unwrap_or_else(|| {
                let all: Vec<&str> = ep.client_calls.iter().map(|c| c.url_raw.as_str()).collect();
                panic!("no client call {url}; got {all:?}")
            })
    };

    // fetch with template literal -> GET /users/{*} -> express endpoint, High.
    let fetch_user = client("/users/${id}");
    assert_eq!(fetch_user.method, HttpMethod::Get);
    assert_eq!(g.functions[fetch_user.caller as usize].name, "loadUser");
    let m = ep
        .matches
        .iter()
        .find(|(cid, _, _)| *cid == fetch_user.id)
        .expect("fetch matched");
    let matched = &ep.endpoints[m.1 as usize];
    assert_eq!(matched.framework, "express");
    assert_eq!(matched.path_norm, "/users/{*}");
    assert_eq!(m.2, gigagraph::types::Confidence::High);

    // axios.post /users -> express POST endpoint.
    let post_user = client("/users");
    assert_eq!(post_user.method, HttpMethod::Post);
    assert!(ep.matches.iter().any(|(cid, eid, _)| *cid == post_user.id
        && ep.endpoints[*eid as usize].framework == "express"
        && ep.endpoints[*eid as usize].method == HttpMethod::Post));

    // Python requests correlate to flask endpoints.
    let get_item = client("/items/9");
    assert!(ep.matches.iter().any(
        |(cid, eid, _)| *cid == get_item.id && ep.endpoints[*eid as usize].framework == "flask"
    ));

    // External URL with no matching endpoint stays unmatched.
    let external = client("https://api.example.com/metrics");
    assert!(ep.matches.iter().all(|(cid, _, _)| *cid != external.id));
}

#[test]
fn resolves_const_paths_and_kotlin_class_prefixes() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // `const BREWS_API = "/brews"; fetch(BREWS_API)` resolves to a literal
    // path and correlates with the slim endpoint.
    let via_const = ep
        .client_calls
        .iter()
        .find(|c| g.functions[c.caller as usize].name == "loadViaConst")
        .expect("const-path fetch detected");
    assert_eq!(via_const.path_norm, "/brews");
    assert!(ep.matches.iter().any(
        |(cid, eid, _)| *cid == via_const.id && ep.endpoints[*eid as usize].framework == "slim"
    ));

    // Spring-Kotlin class-level @RequestMapping prefix joins method routes.
    find(ep, HttpMethod::Get, "/billing/invoices/{*}");
}

#[test]
fn detects_jaxrs_and_declarative_client_interfaces() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // JAX-RS (Java, jakarta.ws.rs): class-level @Path("fleets") prefix +
    // bare verb markers + method-level @Path composition. Prefix joins stay
    // Heuristic (ride-along mis-key caveat, same as Spring).
    let list = find(ep, HttpMethod::Get, "/fleets");
    assert_eq!(list.framework, "jaxrs");
    assert_eq!(list.confidence, Confidence::Heuristic);
    assert_eq!(g.functions[list.handler.unwrap() as usize].name, "list");
    let get = find(ep, HttpMethod::Get, "/fleets/{*}");
    assert_eq!(g.functions[get.handler.unwrap() as usize].name, "get");
    find(ep, HttpMethod::Post, "/fleets");
    find(ep, HttpMethod::Delete, "/fleets/{*}");

    // JAX-RS on Kotlin: marker annotations (@GET) are a distinct capture
    // shape from @GetMapping("/x") — both must land.
    let kt = find(ep, HttpMethod::Get, "/barrels");
    assert_eq!(kt.framework, "jaxrs");
    assert_eq!(g.functions[kt.handler.unwrap() as usize].name, "list");
    find(ep, HttpMethod::Get, "/barrels/{*}");
    find(ep, HttpMethod::Post, "/barrels");

    // MicroProfile @RegisterRestClient interface (javax.ws.rs, marker form):
    // mapped methods are OUTBOUND calls, never routes.
    assert!(
        ep.endpoints.iter().all(|e| !e.path_norm.starts_with("/depots")),
        "rest-client interface methods must not become endpoints"
    );
    let depot = ep
        .client_calls
        .iter()
        .find(|c| c.path_norm == "/depots/{*}")
        .expect("rest-client interface method recorded as client call");
    assert_eq!(depot.library, "rest-client");
    assert_eq!(depot.method, HttpMethod::Get);
    assert_eq!(g.functions[depot.caller as usize].name, "byId");

    // OpenFeign @FeignClient interface with legacy-form @RequestMapping
    // methods: same client routing, library "feign".
    assert!(
        ep.endpoints.iter().all(|e| !e.path_norm.starts_with("/cargo")),
        "feign interface methods must not become endpoints"
    );
    let cargo = ep
        .client_calls
        .iter()
        .find(|c| c.library == "feign" && c.method == HttpMethod::Put)
        .expect("feign PUT recorded as client call");
    assert_eq!(cargo.path_norm, "/cargo/{*}");
    assert_eq!(g.functions[cargo.caller as usize].name, "updateCargo");
    assert!(
        ep.client_calls
            .iter()
            .any(|c| c.library == "feign" && c.method == HttpMethod::Get)
    );
}

#[test]
fn detects_spring_multi_path_annotations() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // @GetMapping({"/a", "/b"}): one route per array member, same handler.
    let a = find(ep, HttpMethod::Get, "/gauges");
    let b = find(ep, HttpMethod::Get, "/gauges.html");
    assert_eq!(a.framework, "spring");
    assert_eq!(a.handler, b.handler);
    assert_eq!(g.functions[a.handler.unwrap() as usize].name, "gauges");

    // Legacy form with value = {..} array + method attribute.
    find(ep, HttpMethod::Get, "/meters");
    find(ep, HttpMethod::Get, "/meters/all");
}

#[test]
fn detects_ktor_slashless_and_pathless_verbs() {
    let index = index();
    let ep = &index.graph.endpoints;
    use gigagraph::types::Confidence;

    // Slashless route + verb segments compose like slashed ones.
    let list = find(ep, HttpMethod::Get, "/crates/list");
    assert_eq!(list.framework, "ktor");
    assert_eq!(list.confidence, Confidence::Heuristic);
    // Pathless verb (`post { }`) binds to the enclosing route's path.
    find(ep, HttpMethod::Post, "/crates");
    // Pathless verb under a nested slashless param route.
    find(ep, HttpMethod::Get, "/crates/{*}");
    // Wrapper lambdas between route levels stay transparent.
    find(ep, HttpMethod::Get, "/vault/keys");
    // Slashless verb with no enclosing route(...) span: not a route.
    assert!(
        ep.endpoints.iter().all(|e| e.path_norm != "/orphan"),
        "bare slashless verb call outside route spans must be ignored"
    );
}

#[test]
fn detects_legacy_php_frameworks() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Symfony 2/3 docblock annotations: @Route + companion @Method({...}).
    let archive = find(ep, HttpMethod::Get, "/reports/archive/{*}");
    assert_eq!(archive.framework, "symfony");
    assert_eq!(
        g.functions[archive.handler.unwrap() as usize].name,
        "archiveAction"
    );
    find(ep, HttpMethod::Head, "/reports/archive/{*}");
    // methods={} inline form inside a docblock.
    let export = find(ep, HttpMethod::Post, "/reports/export");
    assert_eq!(
        g.functions[export.handler.unwrap() as usize].name,
        "exportAction"
    );

    // Pre-5.3 Laravel string handler 'Controller@method' resolves.
    let cart = find(ep, HttpMethod::Get, "/cart/{*}");
    assert_eq!(cart.framework, "laravel");
    assert_eq!(cart.confidence, Confidence::High);
    assert_eq!(
        g.functions[cart.handler.unwrap() as usize].qualified_name,
        "OldShopController::OldShopController::showCart"
    );
}

#[test]
fn detects_silex_routes() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Verb calls on $app with Silex import evidence.
    let list = find(ep, HttpMethod::Get, "/gnomes");
    assert_eq!(list.framework, "silex");
    assert_eq!(list.confidence, Confidence::High);
    // 'GnomeController::listGnomes' string handler resolves cross-file.
    assert_eq!(
        g.functions[list.handler.unwrap() as usize].name,
        "listGnomes"
    );
    let create = find(ep, HttpMethod::Post, "/gnomes");
    assert_eq!(
        g.functions[create.handler.unwrap() as usize].name,
        "createGnome"
    );

    // Closure handler: endpoint exists, handler unresolved.
    let update = find(ep, HttpMethod::Put, "/gnomes/{*}");
    assert_eq!(update.framework, "silex");
    assert!(update.handler.is_none());

    // $app->match(...) maps to ANY.
    find(ep, HttpMethod::Any, "/gnomes/ping");

    // Controller collection routes pick up the $app->mount prefix and are
    // honest Heuristic (non-$app receiver + joined prefix).
    let hats = find(ep, HttpMethod::Get, "/workshop/hats");
    assert_eq!(hats.framework, "silex");
    assert_eq!(hats.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Delete, "/workshop/hats/{*}");
}

#[test]
fn detects_silex_legacy_method_chains() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Silex 1.x `$app->match(...)->method('GET|POST')`: the chained
    // restriction replaces ANY with one endpoint per listed verb.
    let toggle_get = find(ep, HttpMethod::Get, "/gnomes/toggle");
    assert_eq!(toggle_get.framework, "silex");
    assert_eq!(toggle_get.confidence, Confidence::High);
    assert_eq!(
        g.functions[toggle_get.handler.unwrap() as usize].name,
        "createGnome"
    );
    find(ep, HttpMethod::Post, "/gnomes/toggle");
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/gnomes/toggle" && e.method == HttpMethod::Any)),
        "->method('GET|POST') chain must replace the ANY row"
    );

    // $controllers->match(...) on a mounted collection: ANY + mount prefix.
    let rename = find(ep, HttpMethod::Any, "/workshop/hats/{*}/rename");
    assert_eq!(rename.framework, "silex");
    assert_eq!(rename.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[rename.handler.unwrap() as usize].name,
        "createGnome"
    );
}

#[test]
fn detects_silex_provider_without_direct_silex_import() {
    let index = index();
    let ep = &index.graph.endpoints;

    // Provider classes that extend a project base provider never import
    // Silex itself — the `*ControllerProvider` import is the evidence.
    let e = find(ep, HttpMethod::Get, "/widget-registry");
    assert_eq!(e.framework, "silex");
}

#[test]
fn detects_silex_provider_with_cross_file_mount() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Zero-`use` provider file: evidence comes from the structural
    // `implements ControllerProviderInterface` edge, not imports. The
    // bootstrap's `$app->mount('/api/crates', new CrateControllerProvider())`
    // prefixes the provider's routes cross-file, and the `''` collection
    // root maps to the mount prefix itself.
    let root = find(ep, HttpMethod::Get, "/api/crates");
    assert_eq!(root.framework, "silex");
    assert_eq!(root.confidence, Confidence::Heuristic);
    // Service-controller string 'crate.controller:listCrates' resolves by
    // unique method name.
    assert_eq!(
        g.functions[root.handler.unwrap() as usize].name,
        "listCrates"
    );
    find(ep, HttpMethod::Put, "/api/crates/{*}");
}

#[test]
fn detects_silex_script_routes_via_composer_evidence() {
    let index = index();
    let ep = &index.graph.endpoints;
    use gigagraph::types::Confidence;

    // Script-style Silex (Skeleton layout): `$app` arrives via require, the
    // file imports only Symfony components — composer.json's silex/silex
    // requirement is the evidence, restricted to the `$app` receiver.
    let e = find(ep, HttpMethod::Get, "/lanterns");
    assert_eq!(e.framework, "silex");
    assert_eq!(e.confidence, Confidence::High);
    find(ep, HttpMethod::Post, "/lanterns/{*}/light");
}

#[test]
fn resolves_silex_array_callable_handlers() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // Kitchen-Edition idiom: `->post('/{id}/seal', [$this, 'sealCrate'])` —
    // the method lives on the provider class itself.
    let e = find(ep, HttpMethod::Post, "/api/crates/{*}/seal");
    assert_eq!(
        g.functions[e.handler.unwrap() as usize].name,
        "sealCrate"
    );
}

#[test]
fn detects_silex_2_1_api_surface() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // Silex 2.1: Silex\Api namespace provider, full Controller chain
    // (assert/convert/value/bind/secure/before), cross-file $app->mount.
    let show = find(ep, HttpMethod::Get, "/v2/ledgers/{*}");
    assert_eq!(show.framework, "silex");
    assert_eq!(
        g.functions[show.handler.unwrap() as usize].name,
        "showLedger"
    );

    // ->method('POST|DELETE') restriction replaces match's ANY.
    find(ep, HttpMethod::Post, "/v2/ledgers/{*}/close");
    find(ep, HttpMethod::Delete, "/v2/ledgers/{*}/close");
    assert!(
        !ep.endpoints
            .iter()
            .any(|e| e.method == HttpMethod::Any && e.path_norm == "/v2/ledgers/{*}/close"),
        "method restriction must replace the ANY row"
    );

    // options() is a first-class verb.
    find(ep, HttpMethod::Options, "/v2/ledgers");

    // Silex 2 nested collection: $controllers->mount('/nested', $sub)
    // composes with the class's own /v2 mount.
    let deep = find(ep, HttpMethod::Get, "/v2/nested/entries");
    assert_eq!(
        g.functions[deep.handler.unwrap() as usize].name,
        "listEntries"
    );
}

#[test]
fn detects_silex_provider_with_qualified_interface() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;

    // `implements \Silex\ControllerProviderInterface` (FQCN, zero imports):
    // the qualified_name hierarchy pattern supplies the evidence edge.
    let e = find(ep, HttpMethod::Get, "/qualified-ledgers");
    assert_eq!(e.framework, "silex");
    assert_eq!(
        g.functions[e.handler.unwrap() as usize].name,
        "listQualified"
    );
}

#[test]
fn detects_symfony5_annotation_class_prefixes() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Class-level docblock @Route("/member") (annotation-era Symfony,
    // validated against symfony/demo v1.7.0) joins every method route via
    // the php.rs comment ride-along; honest Heuristic.
    let pref = find(ep, HttpMethod::Get, "/member/preferences");
    assert_eq!(pref.framework, "symfony");
    assert_eq!(pref.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[pref.handler.unwrap() as usize].name,
        "preferences"
    );

    // methods="GET|POST" pipe-string form splits into one row per verb and
    // must not leave an ANY row behind.
    find(ep, HttpMethod::Post, "/member/preferences");
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/member/preferences" && e.method == HttpMethod::Any)),
        "pipe methods form must replace the ANY row"
    );

    // requirements={}/defaults={} and inline `{id<\d+>}` requirements leave
    // the path intact (the regex segment folds to {*}).
    let badge = find(ep, HttpMethod::Get, "/member/badges/{*}");
    assert_eq!(g.functions[badge.handler.unwrap() as usize].name, "badge");
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/member/badges/{*}" && e.method == HttpMethod::Any)),
        "brace methods form must replace the ANY row"
    );

    // The class-level prefix itself must be a prefix, not an endpoint.
    assert!(
        ep.endpoints.iter().all(|e| e.path_norm != "/member"),
        "class-level @Route must not become its own endpoint"
    );

    // Method-level docblock routes in prefix-less classes stay High.
    assert_eq!(
        find(ep, HttpMethod::Post, "/reports/export").confidence,
        Confidence::High
    );
}

#[test]
fn detects_laravel8_route_shapes() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Mid-chain prefix: Route::middleware([...])->prefix('/setup')->group()
    // (crater's installation block) — the prefix call is receiver-less but
    // shares the chain-start byte with the group.
    let steps = find(ep, HttpMethod::Get, "/setup/steps");
    assert_eq!(steps.framework, "laravel");
    assert_eq!(steps.confidence, Confidence::Heuristic);

    // Chained verb: Route::middleware('auth')->get('/whoami', ...) — the
    // Laravel 8 skeleton's api.php shape.
    let who = find(ep, HttpMethod::Get, "/whoami");
    assert_eq!(who.framework, "laravel");
    assert_eq!(who.confidence, Confidence::High);

    // Slash-less URI (Route::post('signin', ...)) — argument 0 is the URI
    // by API contract.
    let signin = find(ep, HttpMethod::Post, "/signin");
    assert_eq!(signin.confidence, Confidence::High);

    // Laravel 8 tuple handler [CouponController::class, 'redeem']: the class
    // const sits below harvest depth, so resolution is by project-unique
    // method name.
    let redeem = find(ep, HttpMethod::Post, "/coupons/redeem");
    let h = &g.functions[redeem.handler.expect("tuple handler resolved") as usize];
    assert_eq!(h.name, "redeem");
    assert_eq!(h.containing_type.as_deref(), Some("CouponController"));

    // Single-action (invokable) controller: Route::get('/x', C::class)
    // resolves to the class's __invoke.
    let ver = find(ep, HttpMethod::Get, "/app-version");
    let h = &g.functions[ver.handler.expect("invokable handler resolved") as usize];
    assert_eq!(h.name, "__invoke");
    assert_eq!(h.containing_type.as_deref(), Some("AppVersionController"));

    // apiResource inside a prefix group: 5-route expansion (no HTML form
    // routes) joined with the enclosing prefix, all Heuristic.
    for (m, p) in [
        (HttpMethod::Get, "/api/v1/coupons"),
        (HttpMethod::Post, "/api/v1/coupons"),
        (HttpMethod::Get, "/api/v1/coupons/{*}"),
        (HttpMethod::Patch, "/api/v1/coupons/{*}"),
        (HttpMethod::Delete, "/api/v1/coupons/{*}"),
    ] {
        let e = find(ep, m, p);
        assert_eq!(e.framework, "laravel");
        assert_eq!(e.confidence, Confidence::Heuristic);
    }
    assert!(
        ep.endpoints.iter().all(|e| {
            e.path_norm != "/api/v1/coupons/new" && e.path_norm != "/api/v1/coupons/{*}/edit"
        }),
        "apiResource must not expand create/edit form routes"
    );
}

#[test]
fn detects_rails_nested_and_namespaced_routes() {
    let index = index();
    let ep = &index.graph.endpoints;
    use gigagraph::types::Confidence;

    // root -> GET /.
    let root = find(ep, HttpMethod::Get, "/");
    assert_eq!(root.framework, "rails");
    assert_eq!(root.confidence, Confidence::High);

    // only: [:index, :show] — no create/update/destroy/form routes.
    find(ep, HttpMethod::Get, "/authors");
    find(ep, HttpMethod::Get, "/authors/{*}");
    assert!(
        ep.endpoints.iter().all(|e| {
            !(e.path_norm == "/authors" && e.method == HttpMethod::Post)
                && e.path_norm != "/authors/new"
                && e.path_norm != "/authors/{*}/edit"
        }),
        "only: [:index, :show] must suppress the other resource routes"
    );

    // member / collection blocks and `on: :member`.
    find(ep, HttpMethod::Get, "/authors/{*}/badges");
    find(ep, HttpMethod::Get, "/authors/featured");
    find(ep, HttpMethod::Get, "/authors/{*}/preview");

    // Nested resources under the parent id, only: [:create].
    let books = find(ep, HttpMethod::Post, "/authors/{*}/books");
    assert_eq!(books.confidence, Confidence::Heuristic);
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/authors/{*}/books" && e.method == HttpMethod::Get)),
        "nested only: [:create] must suppress index"
    );

    // namespace :admin prefixes resources and verb routes.
    assert_eq!(find(ep, HttpMethod::Get, "/admin/tools").framework, "rails");
    find(ep, HttpMethod::Get, "/admin/metrics");

    // scope '/api' path prefix.
    find(ep, HttpMethod::Get, "/api/uptime");

    // match via: [:get, :post] -> one row per verb, no ANY row.
    find(ep, HttpMethod::Get, "/archive/import");
    find(ep, HttpMethod::Post, "/archive/import");
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/archive/import" && e.method == HttpMethod::Any)),
        "via: list must replace the ANY row"
    );

    // Sinatra namespace '/wiki' (sinatra-namespace, gollum shape).
    let wiki = find(ep, HttpMethod::Get, "/wiki/pages");
    assert_eq!(wiki.framework, "sinatra");
    assert_eq!(wiki.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Post, "/wiki/pages/{*}/rename");
    // Un-namespaced sinatra routes keep their bare paths and High.
    assert_eq!(find(ep, HttpMethod::Get, "/ping").confidence, Confidence::High);
}

#[test]
fn detects_grape_dsl() {
    let index = index();
    let ep = &index.graph.endpoints;
    use gigagraph::types::Confidence;

    // grape_api.rb carries no `require 'grape'` — the evidence is the
    // `< Grape::API` scoped-superclass hierarchy edge. Everything composes
    // as /prefix/version/nesting/segment and stays Heuristic (class-scoped
    // DSL read file-wide).
    let show = find(ep, HttpMethod::Get, "/api/v1/orders/{*}");
    assert_eq!(show.framework, "grape");
    assert_eq!(show.confidence, Confidence::Heuristic);

    // Path-less `post do ... end` routes at the resource root.
    let create = find(ep, HttpMethod::Post, "/api/v1/orders");
    assert_eq!(create.framework, "grape");

    // route_param :id nesting adds a param segment.
    find(ep, HttpMethod::Get, "/api/v1/orders/{*}/receipt");

    // namespace :admin nesting.
    let stats = find(ep, HttpMethod::Get, "/api/v1/admin/stats");
    assert_eq!(stats.framework, "grape");

    // Helper calls (`find_order(...)`, `desc '...'`) must not become routes:
    // every grape row is one of the four above.
    assert_eq!(
        ep.endpoints.iter().filter(|e| e.framework == "grape").count(),
        4
    );
}

#[test]
fn detects_aspnet_controller_tokens_and_map_groups() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // [Route("api/[controller]")] class prefix + token substitution
    // (aspnetcore-realworld / eShopOnWeb shapes).
    let list = find(ep, HttpMethod::Get, "/api/gadgetinventory");
    assert_eq!(list.framework, "aspnet");
    assert_eq!(list.confidence, Confidence::High);
    assert_eq!(g.functions[list.handler.unwrap() as usize].name, "List");
    let found = find(ep, HttpMethod::Get, "/api/gadgetinventory/{*}");
    assert_eq!(g.functions[found.handler.unwrap() as usize].name, "Find");
    find(ep, HttpMethod::Post, "/api/gadgetinventory/bulk");
    // Method-level [Route] without a verb attribute -> ANY.
    let export = find(ep, HttpMethod::Any, "/api/gadgetinventory/export");
    assert_eq!(g.functions[export.handler.unwrap() as usize].name, "Export");

    // [controller]/[action] template inherited from a method-less base
    // controller (eShopOnWeb BaseApiController shape) — Heuristic.
    let sizes = find(ep, HttpMethod::Get, "/api/wrench/sizes");
    assert_eq!(sizes.confidence, Confidence::Heuristic);
    assert_eq!(g.functions[sizes.handler.unwrap() as usize].name, "Sizes");

    // Absolute method templates stay untouched and High.
    assert_eq!(
        find(ep, HttpMethod::Get, "/api/users/{*}").confidence,
        Confidence::High
    );

    // Minimal APIs: var-bound MapGroup borrowed file-wide (Heuristic),
    // slash-less patterns, chained MapGroup, unprefixed app.MapGet High.
    let lite = find(ep, HttpMethod::Get, "/minapi/orders-lite");
    assert_eq!(lite.framework, "aspnet");
    assert_eq!(lite.confidence, Confidence::Heuristic);
    find(ep, HttpMethod::Post, "/minapi/orders-lite");
    find(ep, HttpMethod::Delete, "/minapi/orders-lite/{*}");
    assert_eq!(
        find(ep, HttpMethod::Get, "/healthz-lite").confidence,
        Confidence::High
    );
    assert!(
        ep.endpoints.iter().all(|e| e.path_norm != "/orders-lite"),
        "grouped Map* calls must not emit unprefixed rows"
    );
}

#[test]
fn detects_rust_router_composition() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // Chained multi-verb method router: each verb binds to ITS route call.
    let post = find(ep, HttpMethod::Post, "/sprocketeers");
    assert_eq!(post.framework, "axum");
    assert_eq!(
        g.functions[post.handler.unwrap() as usize].name,
        "add_sprocketeer"
    );
    let get = find(ep, HttpMethod::Get, "/sprocketeers");
    assert_eq!(
        g.functions[get.handler.unwrap() as usize].name,
        "list_sprocketeers"
    );

    // nest("/panel", panel_routes()) cross-function prefix, Heuristic.
    let health = find(ep, HttpMethod::Get, "/panel/health");
    assert_eq!(health.framework, "axum");
    assert_eq!(health.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[health.handler.unwrap() as usize].name,
        "panel_health"
    );
    assert!(
        ep.endpoints
            .iter()
            .all(|e| !(e.path_norm == "/health" && e.framework == "axum")),
        "nested router must not keep its unprefixed row"
    );

    // merge(gauges_router()) keeps the path, resolves the handler, High.
    let gauge = find(ep, HttpMethod::Delete, "/gauges/{*}");
    assert_eq!(gauge.confidence, Confidence::High);
    assert_eq!(g.functions[gauge.handler.unwrap() as usize].name, "drop_gauge");

    // actix scope prefix joins a `.service(handler)`d attribute route.
    let ledger = find(ep, HttpMethod::Get, "/portal/ledger-lines");
    assert_eq!(ledger.framework, "actix");
    assert_eq!(ledger.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[ledger.handler.unwrap() as usize].name,
        "ledger_lines"
    );
    assert!(
        ep.endpoints.iter().all(|e| e.path_norm != "/ledger-lines"),
        "scoped service must not keep its unprefixed row"
    );

    // web::resource(...).route(web::VERB().to(handler)) under a scope.
    let crates = find(ep, HttpMethod::Get, "/api2/crates2");
    assert_eq!(crates.framework, "actix");
    assert_eq!(
        g.functions[crates.handler.unwrap() as usize].name,
        "list_crates2"
    );
    let add = find(ep, HttpMethod::Post, "/api2/crates2");
    assert_eq!(g.functions[add.handler.unwrap() as usize].name, "add_crate2");

    // Bare .route("/x", web::get().to(h)) stays unprefixed and High.
    let ping = find(ep, HttpMethod::Get, "/direct-ping");
    assert_eq!(ping.confidence, Confidence::High);
    assert_eq!(
        g.functions[ping.handler.unwrap() as usize].name,
        "direct_ping"
    );

    // Unregistered attribute routes keep their bare paths and High.
    assert_eq!(
        find(ep, HttpMethod::Get, "/invoices/{*}").confidence,
        Confidence::High
    );
}

#[test]
fn generic_php_tier_catches_unknown_routers() {
    let index = index();
    let ep = &index.graph.endpoints;
    let g = &index.graph;
    use gigagraph::types::Confidence;

    // No framework evidence at all: bare $router verbs land as Heuristic
    // "php" rows, with string handlers still resolving.
    let e = find(ep, HttpMethod::Get, "/beacons");
    assert_eq!(e.framework, "php");
    assert_eq!(e.confidence, Confidence::Heuristic);
    assert_eq!(
        g.functions[e.handler.unwrap() as usize].name,
        "listBeacons"
    );
    find(ep, HttpMethod::Post, "/beacons");
    find(ep, HttpMethod::Put, "/beacons/{*}");

    // Fat-Free style verb-in-string: $f3->route('GET|POST /signals', h).
    find(ep, HttpMethod::Get, "/signals");
    find(ep, HttpMethod::Post, "/signals");

    // Client-ish receivers stay out.
    assert!(
        !ep.endpoints.iter().any(|e| e.path_norm == "/not-a-route"),
        "$client->get must not become an endpoint"
    );
}
