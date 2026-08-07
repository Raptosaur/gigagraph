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
