//! API endpoint + client-call detection and correlation: HTTP routes and
//! RPC-style operations (SOAP, XML-RPC, JSON-RPC, gRPC services).
//!
//! Framework knowledge lives here, in Rust tables, not in tree-sitter
//! queries: the generic extractor already surfaces every call with distilled
//! literal arguments (`ArgLit`) and every decorator/annotation
//! (`RawDecoration`); this module interprets those shapes, evidence-gated by
//! each file's imports so `cache.get("/key")` doesn't become an HTTP call.
//!
//! Every rule is gated on at least one of: import evidence, receiver shape,
//! or a file naming convention (Django `urls.py`, Rails `config/routes.rb`),
//! and carries an honest `Confidence`.

use crate::extract::{LitKind, RawCall, RawDecoration};
use crate::types::{Confidence, FileInfo, FunctionInfo, Lang};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

/// Protocol family of a published operation / outbound call. `Http` rows
/// carry real paths; RPC-style rows carry an operation (or gRPC service)
/// name in the path fields and correlate by name equality, not path shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ApiKind {
    #[default]
    Http,
    Soap,
    XmlRpc,
    JsonRpc,
    Grpc,
    /// GraphQL operations (AppSync resolvers, schema fields). Correlates by
    /// operation-name equality like the RPC kinds, not by path shape.
    Graphql,
}

impl ApiKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiKind::Http => "http",
            ApiKind::Soap => "soap",
            ApiKind::XmlRpc => "xml-rpc",
            ApiKind::JsonRpc => "json-rpc",
            ApiKind::Grpc => "grpc",
            ApiKind::Graphql => "graphql",
        }
    }

    pub fn from_name(s: &str) -> Option<ApiKind> {
        Some(match s.to_ascii_lowercase().as_str() {
            "http" | "rest" => ApiKind::Http,
            "soap" => ApiKind::Soap,
            "xml-rpc" | "xmlrpc" => ApiKind::XmlRpc,
            "json-rpc" | "jsonrpc" => ApiKind::JsonRpc,
            "grpc" => ApiKind::Grpc,
            "graphql" | "gql" => ApiKind::Graphql,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    /// Statically unknowable (Django views, bare `route`/`HandleFunc`).
    Any,
}

impl HttpMethod {
    pub fn from_name(s: &str) -> Option<HttpMethod> {
        Some(match s.to_ascii_lowercase().as_str() {
            "get" => HttpMethod::Get,
            "post" => HttpMethod::Post,
            "put" => HttpMethod::Put,
            "delete" => HttpMethod::Delete,
            "patch" => HttpMethod::Patch,
            "head" => HttpMethod::Head,
            "options" => HttpMethod::Options,
            "all" | "any" | "route" | "match" => HttpMethod::Any,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Any => "ANY",
        }
    }

    pub fn compatible(a: HttpMethod, b: HttpMethod) -> bool {
        a == b || a == HttpMethod::Any || b == HttpMethod::Any
    }
}

/// Do two normalized paths refer to the same route shape?
pub fn paths_unify(a: &str, b: &str) -> bool {
    unify(&segs(a), &segs(b))
}

/// A route the indexed code publishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: u32,
    pub kind: ApiKind,
    pub method: HttpMethod,
    pub path_raw: String,
    pub path_norm: String,
    pub framework: String,
    pub file_id: u32,
    pub line: u32,
    /// Handler function, when statically resolvable.
    pub handler: Option<u32>,
    pub confidence: Confidence,
}

/// An outbound HTTP call the indexed code makes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCall {
    pub id: u32,
    pub kind: ApiKind,
    pub method: HttpMethod,
    pub url_raw: String,
    pub path_norm: String,
    pub library: String,
    pub caller: u32,
    pub file_id: u32,
    pub line: u32,
    /// How solid the detection itself is (receiver-shape-only rules like
    /// XHR/jQuery are Heuristic); caps correlation confidence.
    pub confidence: Confidence,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct EndpointIndex {
    pub endpoints: Vec<Endpoint>,
    pub client_calls: Vec<ClientCall>,
    /// (client_call id, endpoint id, confidence)
    pub matches: Vec<(u32, u32, Confidence)>,
}

const VERBS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

/// Silex evidence: the framework import itself, or any `*ControllerProvider`
/// import — provider classes routinely extend a project base provider and
/// never import Silex directly (the interface import lives in the base). A
/// same-namespace `extends` with no `use` at all stays undetectable.
const SILEX_EV: &[&str] = &["silex", "controllerprovider"];

/// Cross-function facts gathered in a pre-pass over the whole file set.
#[derive(Default)]
struct FileCtx {
    /// file id -> baseURL literal from an `axios.create({ baseURL: ... })`.
    axios_base: FxHashMap<u32, String>,
    /// (file id, class name) -> path prefix from NestJS `@Controller("x")`.
    controller_prefix: FxHashMap<(u32, String), String>,
    /// (file id, class name) -> class-level Spring `@RequestMapping` prefix.
    /// Both Java and Kotlin queries let the class annotation ride along to
    /// the class's first method (see `src/lang/java.rs` and the matching
    /// kotlin.rs pattern); the pre-pass re-keys it by `containing_type`.
    spring_prefix: FxHashMap<(u32, String), String>,
    /// (file id, class name) -> class-level JAX-RS `@Path` prefix (Quarkus /
    /// Micronaut / plain JEE). Same ride-along mechanics as `spring_prefix`,
    /// gated on `javax.ws.rs` / `jakarta.ws.rs` imports in the pre-pass.
    jaxrs_prefix: FxHashMap<(u32, String), String>,
    /// (file id, type name) of interfaces that are declarative HTTP CLIENTS:
    /// Spring Cloud OpenFeign `@FeignClient` and MicroProfile
    /// `@RegisterRestClient`. Mapping annotations on their methods describe
    /// outbound requests, so detect_spring/detect_jaxrs route them to
    /// `client_calls` instead of `endpoints`.
    client_iface: FxHashSet<(u32, String)>,
    /// Target file id -> (mounting file id, mount prefix) from
    /// `app.use("/api", importedRouter)` / `fastify.register(routes,
    /// { prefix })` in ANOTHER file, applied to endpoints declared in the
    /// target file. The mounting file id lets `file_mount_prefix` compose
    /// nested mounts (`app.use('/api/v1', routes)` -> routes file mounts
    /// deeper routers). `None` = conflicting prefixes seen (ambiguous).
    mount_file: FxHashMap<u32, Option<(u32, String)>>,
    /// (file id, local router ident) -> mount prefix from a same-file
    /// `app.use("/api", router)`, applied to endpoints registered on that
    /// receiver. `None` = ambiguous.
    mount_recv: FxHashMap<(u32, String), Option<String>>,
    /// (file id, const name) -> value from PHP `define('K', '/v')` — the only
    /// const-definition shape visible to detect(): the extractor surfaces
    /// calls, not plain assignments, so JS/TS `const API = "/x"`, PHP
    /// `const API = '/x'`, and Java `static final String BASE = "/x"` never
    /// reach the extraction stream. Documented phase-3 gap: for those, an
    /// Ident at the URL position has no definition to look up, and the call
    /// is skipped exactly as before.
    php_consts: FxHashMap<(u32, String), String>,
    /// Provider class name -> mount prefix from `$app->mount('/p',
    /// new XControllerProvider())` — cross-file: routes registered inside
    /// that class's methods (connect()) join the prefix. `None` = mounted at
    /// conflicting prefixes.
    mount_class: FxHashMap<String, Option<String>>,
    /// file id -> the file's single CDK Lambda declaration whose handler
    /// resolved to an indexed function. `Some(fn)` only when the file
    /// declares EXACTLY ONE lambda and it resolved; `None` records "lambdas
    /// exist here but the binding is ambiguous or unresolved". Real dataflow
    /// (`const fn = new Function(...)` then `LambdaIntegration(fn)`) is
    /// invisible in the harvested literals, so `detect_cdk` borrows this
    /// file-scope binding for its routes — always Heuristic, because
    /// single-lambda-per-file is an assumption, not knowledge.
    cdk_lambda: FxHashMap<u32, Option<u32>>,
    /// NestJS `app.setGlobalPrefix("api")` (main.ts): a PROJECT-wide URI
    /// prefix joined onto every nestjs route. Outer `None` = never seen;
    /// `Some(None)` = conflicting prefixes across files (no join).
    nest_prefix: Option<Option<String>>,
}

/// Compose a file's transitive mount prefix, outermost first:
/// `app.use('/v1', routes)` (app.js) + routes file re-exporting deeper
/// mounts. Depth-capped, cycle-safe; any ambiguous (`None`) link kills the
/// whole join — a partial prefix is worse than none.
fn file_mount_prefix(ctx: &FileCtx, fid: u32) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = fid;
    for _ in 0..4 {
        match ctx.mount_file.get(&cur) {
            Some(Some((from, p))) => {
                parts.push(p);
                if *from == cur {
                    break;
                }
                cur = *from;
            }
            Some(None) => return None,
            None => break,
        }
    }
    (!parts.is_empty()).then(|| {
        parts
            .iter()
            .rev()
            .fold(String::new(), |acc, p| join_prefix(&acc, p))
    })
}

/// CDK import evidence: v2 monopackage + v1 scoped packages (JS/TS) and the
/// Python module (`from aws_cdk import ...` surfaces path "aws_cdk").
const CDK_EV_JS: &[&str] = &["aws-cdk-lib", "@aws-cdk/"];
const CDK_EV_PY: &[&str] = &["aws_cdk"];

/// Record a mount prefix, degrading to `None` when the same key is mounted
/// at two different prefixes (ambiguous joins are worse than no join).
fn upsert_mount<K: std::hash::Hash + Eq>(
    map: &mut FxHashMap<K, Option<String>>,
    key: K,
    prefix: String,
) {
    map.entry(key)
        .and_modify(|v| {
            if v.as_deref() != Some(prefix.as_str()) {
                *v = None;
            }
        })
        .or_insert(Some(prefix));
}

/// `mount_file` variant of `upsert_mount`: same ambiguity degradation, but
/// the value carries the mounting file so nested mounts can compose. Two
/// mounts agreeing on the prefix keep the first origin file.
fn upsert_mount_file(
    map: &mut FxHashMap<u32, Option<(u32, String)>>,
    target: u32,
    from: u32,
    prefix: String,
) {
    map.entry(target)
        .and_modify(|v| {
            if v.as_ref().map(|(_, p)| p.as_str()) != Some(prefix.as_str()) {
                *v = None;
            }
        })
        .or_insert(Some((from, prefix)));
}

pub fn detect(
    files: &[FileInfo],
    functions: &[FunctionInfo],
    raw_calls: &[Vec<RawCall>],
    decorations: &[Vec<RawDecoration>],
    name_index: &FxHashMap<String, Vec<u32>>,
    file_hierarchy: &[Vec<(String, String)>],
    project_evidence: &str,
) -> EndpointIndex {
    let project_evidence = project_evidence.to_ascii_lowercase();
    let mut idx = EndpointIndex::default();

    // Per-file import evidence, lowercased: import paths + attributed
    // packages, PLUS structural evidence — the base names each file's types
    // implement/extend, prefixed "implements:". Zero-`use` files (global-
    // namespace Silex providers) carry no import evidence at all; a
    // `class X implements ControllerProviderInterface` clause is just as
    // strong a signal and survives the missing imports.
    let evidence: Vec<String> = files
        .iter()
        .map(|f| {
            let mut s = String::new();
            for imp in &f.imports {
                s.push_str(&imp.path.to_ascii_lowercase());
                s.push('\n');
                if let Some(p) = &imp.external_package {
                    s.push_str(&p.to_ascii_lowercase());
                    s.push('\n');
                }
            }
            if let Some(edges) = file_hierarchy.get(f.id as usize) {
                for (_, base) in edges {
                    s.push_str("implements:");
                    s.push_str(&base.to_ascii_lowercase());
                    s.push('\n');
                }
            }
            s
        })
        .collect();
    let has = |file_id: u32, needles: &[&str]| -> bool {
        let ev = &evidence[file_id as usize];
        needles.iter().any(|n| ev.contains(n))
    };

    // ---- Pre-pass: file-level context ----
    let mut ctx = FileCtx::default();
    // file id -> one entry per CDK Lambda declaration seen (resolved or not);
    // folded into `ctx.cdk_lambda` after the loop.
    let mut cdk_decls: FxHashMap<u32, Vec<Option<u32>>> = FxHashMap::default();
    for func in functions {
        let fid = func.file_id;
        let file = &files[fid as usize];
        let calls = &raw_calls[func.id as usize];
        match file.language {
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
                if has(fid, &["axios"]) {
                    for call in calls {
                        if call.name == "create" && call.receiver.as_deref() == Some("axios") {
                            if let Some(base) = str_lit_by_key(call, &["baseURL", "baseUrl"]) {
                                ctx.axios_base.entry(fid).or_insert(base);
                            }
                        }
                    }
                }
                // NestJS class prefixes. True class-level decorations are
                // routed by the extractor to `ExtractedFile.type_decorations`,
                // which is not plumbed into detect(); instead the TS query
                // captures class decorators WITHOUT `@deco.type`, so the
                // extractor associates them with the nearest following
                // function — the class's first method. Keyed by that method's
                // `containing_type`, the prefix is recovered for every
                // sibling method. Known limit: a decorated class with no
                // methods leaks its decoration to the next function in the
                // file.
                if has(fid, &["@nestjs"]) {
                    if let Some(t) = &func.containing_type {
                        for d in &decorations[func.id as usize] {
                            if d.name == "Controller" {
                                let prefix = first_deco_str(d).unwrap_or_default();
                                ctx.controller_prefix
                                    .entry((fid, t.clone()))
                                    .or_insert(prefix);
                            }
                        }
                    }
                }
                // Express/Koa mounts: `app.use("/api", thing)`. An
                // import-bound `thing` resolving to an indexed file prefixes
                // the endpoints DECLARED in that file; an unbound (local,
                // usually `Router()`) `thing` prefixes endpoints registered
                // on that receiver in this file. Mounted external middleware
                // (`serve-static` etc.) resolves to no file and is ignored.
                // Local non-router idents mounted at a path can register a
                // receiver prefix that never matches an endpoint — harmless.
                if has(fid, CDK_EV_JS) {
                    collect_cdk_lambdas(
                        file,
                        files,
                        calls,
                        functions,
                        name_index,
                        cdk_decls.entry(fid).or_default(),
                    );
                }
                if has(fid, &["express", "@koa/router", "koa-router"]) {
                    for call in calls {
                        // Koa `new Router({ prefix: '/api' })`: the ctor's
                        // options object surfaces as an Ident-key/Str-value
                        // window and `assigned_to` names the router variable,
                        // so routes registered on that receiver join the
                        // prefix exactly like a same-file mount.
                        if call.name == "Router" {
                            if let (Some(prefix), Some(var)) =
                                (str_lit_by_key(call, &["prefix"]), &call.assigned_to)
                            {
                                upsert_mount(&mut ctx.mount_recv, (fid, var.clone()), prefix);
                            }
                            continue;
                        }
                        if call.name != "use" {
                            continue;
                        }
                        let Some(prefix) = first_path_lit(call) else {
                            continue;
                        };
                        let Some(ident) = call
                            .arg_lits
                            .iter()
                            .find(|l| l.kind == LitKind::Ident && l.key.is_none())
                        else {
                            continue;
                        };
                        // `router.routes()` (Koa) harvests as `router.routes`.
                        let base = ident.text.split('.').next().unwrap_or("");
                        if base.is_empty() {
                            continue;
                        }
                        match file
                            .imports
                            .iter()
                            .find(|i| i.names.iter().any(|n| n == base))
                        {
                            Some(imp) => {
                                if let Some(target) = imp.resolved_file {
                                    upsert_mount_file(&mut ctx.mount_file, target, fid, prefix);
                                }
                            }
                            None => {
                                upsert_mount(&mut ctx.mount_recv, (fid, base.to_string()), prefix);
                            }
                        }
                    }
                }
                // Fastify plugin prefixes: `fastify.register(routes,
                // { prefix: '/v1' })` — an import-bound plugin ident mounted
                // at a prefix maps the plugin's defining file, exactly like
                // an Express cross-file `app.use` mount.
                if has(fid, &["fastify"]) {
                    for call in calls {
                        if call.name != "register" {
                            continue;
                        }
                        let Some(prefix) = str_lit_by_key(call, &["prefix"]) else {
                            continue;
                        };
                        let Some(target) = call
                            .arg_lits
                            .iter()
                            .find(|l| l.kind == LitKind::Ident && l.key.is_none())
                            .and_then(|l| {
                                let base = l.text.split('.').next()?;
                                file.imports
                                    .iter()
                                    .find(|i| i.names.iter().any(|n| n == base))
                                    .and_then(|i| i.resolved_file)
                            })
                        else {
                            continue;
                        };
                        upsert_mount_file(&mut ctx.mount_file, target, fid, prefix);
                    }
                }
                // NestJS `app.setGlobalPrefix('api')` (main.ts): a global URI
                // prefix every controller route joins. Cross-file by nature,
                // so it lives on the ctx, not a per-file map.
                if has(fid, &["@nestjs"]) {
                    for call in calls {
                        if call.name != "setGlobalPrefix" {
                            continue;
                        }
                        let Some(p) = first_str_lit(call) else {
                            continue;
                        };
                        match &ctx.nest_prefix {
                            None => ctx.nest_prefix = Some(Some(p)),
                            Some(Some(existing)) if *existing != p => {
                                ctx.nest_prefix = Some(None);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Lang::Java | Lang::Kotlin => {
                // Spring class-level `@RequestMapping("/prefix")`: rides
                // along to the class's first method (see `src/lang/java.rs`
                // and the matching kotlin.rs pattern), recognizable there
                // because its line precedes the method declaration while
                // method-level annotations sit inside it. Keyed by
                // `containing_type` like the NestJS trick above.
                if let (true, Some(t)) = (has(fid, &["org.springframework"]), &func.containing_type)
                {
                    for d in &decorations[func.id as usize] {
                        if d.name == "RequestMapping" && d.line < func.start_line {
                            ctx.spring_prefix
                                .entry((fid, t.clone()))
                                .or_insert_with(|| spring_path_arg(d));
                        }
                    }
                }
                // JAX-RS class-level `@Path("prefix")`: same ride-along
                // shape, gated on the ws.rs import families so a stray
                // `@Path` annotation from another library can't seed a
                // prefix.
                if let (true, Some(t)) = (
                    has(fid, &["javax.ws.rs", "jakarta.ws.rs"]),
                    &func.containing_type,
                ) {
                    for d in &decorations[func.id as usize] {
                        if d.name == "Path" && d.line < func.start_line {
                            ctx.jaxrs_prefix
                                .entry((fid, t.clone()))
                                .or_insert_with(|| spring_path_arg(d));
                        }
                    }
                }
                // Declarative HTTP-client interfaces: OpenFeign
                // `@FeignClient(...)` and MicroProfile `@RegisterRestClient`
                // ride along from the interface declaration exactly like the
                // prefixes above. Their mapped methods describe OUTBOUND
                // calls, so the detectors below must not publish them as
                // routes.
                if let Some(t) = &func.containing_type {
                    for d in &decorations[func.id as usize] {
                        if (d.name == "FeignClient" || d.name == "RegisterRestClient")
                            && d.line < func.start_line
                        {
                            ctx.client_iface.insert((fid, t.clone()));
                        }
                    }
                }
            }
            Lang::Php => {
                // Symfony class-level docblock `@Route("/prefix")` (annotation
                // era, Symfony 2-5 / PHP 7): the php.rs query captures the
                // class docblock COMMENT as a decoration riding along to the
                // class's first method (name = raw comment text — see
                // src/lang/php.rs). Recovered here into `controller_prefix`
                // keyed by containing_type, NestJS-style, so every sibling
                // method joins the prefix. Class-level PHP8 `#[Route]`
                // ATTRIBUTES are not captured (documented gap: they would be
                // indistinguishable from the method-level docblock-synthesized
                // "Route" decorations that also precede the method line).
                if has(fid, &["symfony", "sensio"]) {
                    if let Some(t) = &func.containing_type {
                        for d in &decorations[func.id as usize] {
                            if let Some(prefix) = php_class_docblock_route(d) {
                                ctx.controller_prefix
                                    .entry((fid, t.clone()))
                                    .or_insert(prefix);
                            }
                        }
                    }
                }
                // `define('NAME', '/path')` is call-shaped, so it is the one
                // const definition the extractor surfaces (see the
                // `php_consts` field doc for what is NOT visible).
                for call in calls {
                    if call.name == "define" && call.receiver.is_none() {
                        let mut strs = call
                            .arg_lits
                            .iter()
                            .filter(|l| l.kind == LitKind::Str && l.key.is_none());
                        if let (Some(k), Some(v)) = (strs.next(), strs.next()) {
                            ctx.php_consts
                                .entry((fid, k.text.clone()))
                                .or_insert_with(|| v.text.clone());
                        }
                    }
                }
                // Silex `$app->mount('/prefix', ...)`, two shapes:
                // - `$app->mount('/p', $collection)` — same-file variable:
                //   reuse the JS mount_recv machinery keyed by receiver
                //   spelling (arg-lit idents drop the `$`, so it's re-added).
                // - `$app->mount('/p', new WidgetControllerProvider())` —
                //   the object creation is a separate uppercase-named RawCall
                //   byte-contained in the mount's range (the new-expression
                //   itself never survives arg-lit classification); map the
                //   provider CLASS to the prefix so its connect() routes in
                //   ANY file pick it up.
                if has(fid, SILEX_EV) || project_evidence.contains("silex/silex") {
                    for call in calls {
                        // Silex 2 ControllerCollection::mount means nested
                        // collections mount on ANY `$` receiver, not just
                        // `$app` ($controllers->mount('/sub', $inner)).
                        if call.name != "mount"
                            || !call.receiver.as_deref().is_some_and(|r| r.starts_with('$'))
                        {
                            continue;
                        }
                        let Some(prefix) = first_path_lit(call) else {
                            continue;
                        };
                        // An uppercase-initial ident is the provider CLASS
                        // (the depth-2 harvest surfaces `new X()`'s name as a
                        // bare Ident); lowercase = a `$collection` variable.
                        let ident = call
                            .arg_lits
                            .iter()
                            .find(|l| l.kind == LitKind::Ident && l.key.is_none());
                        match ident {
                            Some(l) if l.text.chars().next().is_some_and(|c| c.is_uppercase()) => {
                                upsert_mount(&mut ctx.mount_class, l.text.clone(), prefix);
                            }
                            Some(l) => {
                                let recv = if l.text.starts_with('$') {
                                    l.text.clone()
                                } else {
                                    format!("${}", l.text)
                                };
                                upsert_mount(&mut ctx.mount_recv, (fid, recv), prefix);
                            }
                            None => {
                                if let Some(ctor) = calls.iter().find(|c| {
                                    c.start_byte > call.start_byte
                                        && c.end_byte <= call.end_byte
                                        && c.name.chars().next().is_some_and(|ch| ch.is_uppercase())
                                }) {
                                    upsert_mount(&mut ctx.mount_class, ctor.name.clone(), prefix);
                                }
                            }
                        }
                    }
                }
            }
            Lang::Python if has(fid, CDK_EV_PY) => {
                collect_cdk_lambdas(
                    file,
                    files,
                    calls,
                    functions,
                    name_index,
                    cdk_decls.entry(fid).or_default(),
                );
            }
            _ => {}
        }
    }
    for (fid, decls) in cdk_decls {
        // Single-lambda-per-file assumption: only an unambiguous, resolved
        // declaration becomes a borrowable binding.
        ctx.cdk_lambda
            .insert(fid, if decls.len() == 1 { decls[0] } else { None });
    }

    for func in functions {
        let file = &files[func.file_id as usize];
        let calls = &raw_calls[func.id as usize];
        let decos = &decorations[func.id as usize];

        detect_server(
            func,
            file,
            calls,
            decos,
            &has,
            functions,
            name_index,
            &ctx,
            &project_evidence,
            &mut idx,
        );
        detect_client(func, file, calls, decos, &has, &ctx, &mut idx);
        detect_rpc_server(
            func, file, calls, decos, &has, functions, name_index, &mut idx,
        );
        detect_rpc_client(func, file, calls, &has, &mut idx);
    }

    idx.matches = correlate(&idx.endpoints, &idx.client_calls);
    idx
}

/// RPC-style published operations: SOAP (JAX-WS, WCF/ASMX, spyne, PHP
/// SoapServer), XML-RPC (Python `SimpleXMLRPCServer`, PHP
/// `xmlrpc_server_register_method`), JSON-RPC (jsonrpcserver, json-rpc-2.0 /
/// jayson `addMethod`), and gRPC service registration (generated
/// `RegisterXServer` / `add_XServicer_to_server` / `addService`). gRPC rows
/// are SERVICE-level — individual methods live in the .proto, which is not
/// indexed. The operation name rides in the path fields (normalized
/// `/lowercased-op`); `method` is always `Any`.
#[allow(clippy::too_many_arguments)]
fn detect_rpc_server(
    func: &FunctionInfo,
    file: &FileInfo,
    calls: &[RawCall],
    decos: &[RawDecoration],
    has: &dyn Fn(u32, &[&str]) -> bool,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;
    match file.language {
        Lang::Java | Lang::Kotlin => {
            // JAX-WS: @WebMethod on the operation (javax legacy AND jakarta).
            // `operationName` overrides the Java method name.
            if has(fid, &["javax.jws", "jakarta.jws"]) {
                for d in decos {
                    if d.name != "WebMethod" || d.line < func.start_line {
                        continue;
                    }
                    let op = d
                        .arg_lits
                        .iter()
                        .find(|l| {
                            l.kind == LitKind::Str && l.key.as_deref() == Some("operationName")
                        })
                        .map(|l| l.text.clone())
                        .unwrap_or_else(|| func.name.clone());
                    push_rpc_op(
                        idx,
                        ApiKind::Soap,
                        op,
                        "jaxws",
                        fid,
                        d.line,
                        Some(func.id),
                        Confidence::High,
                    );
                }
            }
            // gRPC-Java: serverBuilder.addService(new GreeterImpl()).
            if has(fid, &["io.grpc"]) {
                for call in calls {
                    if call.name != "addService" {
                        continue;
                    }
                    let Some(service) = call
                        .arg_lits
                        .iter()
                        .find(|l| l.kind == LitKind::Ident)
                        .map(|l| grpc_service_from_ident(&l.text))
                    else {
                        continue;
                    };
                    push_rpc_op(
                        idx,
                        ApiKind::Grpc,
                        service,
                        "grpc",
                        fid,
                        call.line,
                        None,
                        Confidence::Heuristic,
                    );
                }
            }
        }
        Lang::CSharp => {
            // WCF [OperationContract] / classic ASMX [WebMethod].
            let wcf = has(fid, &["system.servicemodel"]);
            let asmx = has(fid, &["system.web.services"]);
            if !wcf && !asmx {
                return;
            }
            for d in decos {
                let framework = match d.name.as_str() {
                    "OperationContract" if wcf => "wcf",
                    "WebMethod" if asmx => "asmx",
                    _ => continue,
                };
                push_rpc_op(
                    idx,
                    ApiKind::Soap,
                    func.name.clone(),
                    framework,
                    fid,
                    d.line,
                    Some(func.id),
                    Confidence::High,
                );
            }
        }
        Lang::Python => {
            // spyne: @rpc / @srpc decorated service methods.
            if has(fid, &["spyne"]) {
                for d in decos {
                    let last = d.name.rsplit('.').next().unwrap_or(&d.name);
                    if matches!(last, "rpc" | "srpc") {
                        push_rpc_op(
                            idx,
                            ApiKind::Soap,
                            func.name.clone(),
                            "spyne",
                            fid,
                            d.line,
                            Some(func.id),
                            Confidence::High,
                        );
                    }
                }
            }
            for call in calls {
                // SimpleXMLRPCServer: server.register_function(fn, 'name').
                // The public name is the string arg when present, else the
                // identifier's own name.
                if call.name == "register_function" && has(fid, &["xmlrpc"]) {
                    let handler = handler_from_ident(call, func, file, functions, name_index);
                    let op = first_str_lit(call).or_else(|| {
                        call.arg_lits
                            .iter()
                            .find(|l| l.kind == LitKind::Ident)
                            .map(|l| l.text.rsplit('.').next().unwrap_or(&l.text).to_string())
                    });
                    if let Some(op) = op {
                        push_rpc_op(
                            idx,
                            ApiKind::XmlRpc,
                            op,
                            "xmlrpc-server",
                            fid,
                            call.line,
                            handler,
                            Confidence::High,
                        );
                    }
                }
                // jsonrpcserver: also exposes `@method` decorators, but the
                // call-shaped registration `methods.add(fn)` is rarer; the
                // decorator form is handled below with the deco loop.
                // grpc: add_GreeterServicer_to_server(GreeterServicer(), srv).
                if has(fid, &["grpc"]) {
                    if let Some(service) = call
                        .name
                        .strip_prefix("add_")
                        .and_then(|s| s.strip_suffix("Servicer_to_server"))
                    {
                        if !service.is_empty() {
                            push_rpc_op(
                                idx,
                                ApiKind::Grpc,
                                service.to_string(),
                                "grpc",
                                fid,
                                call.line,
                                None,
                                Confidence::High,
                            );
                        }
                    }
                }
            }
            // jsonrpcserver: @method decorated handlers.
            if has(fid, &["jsonrpcserver"]) {
                for d in decos {
                    if d.name.rsplit('.').next() == Some("method") {
                        push_rpc_op(
                            idx,
                            ApiKind::JsonRpc,
                            func.name.clone(),
                            "jsonrpcserver",
                            fid,
                            d.line,
                            Some(func.id),
                            Confidence::High,
                        );
                    }
                }
            }
        }
        Lang::Php => {
            // SoapServer / XML-RPC are PHP built-ins: no import evidence
            // exists. Gates are structural instead: a `SoapServer`
            // constructor call visible in the same function for addFunction/
            // setClass, and the globally-unique built-in name for
            // xmlrpc_server_register_method.
            let soap_server_here = calls.iter().any(|c| c.name == "SoapServer");
            for call in calls {
                match call.name.as_str() {
                    "addFunction" if soap_server_here => {
                        if let Some(op) = first_str_lit(call) {
                            let handler = name_index.get(&op).and_then(|ids| {
                                let hits: Vec<u32> = ids
                                    .iter()
                                    .copied()
                                    .filter(|&id| !functions[id as usize].is_toplevel)
                                    .collect();
                                (hits.len() == 1).then(|| hits[0])
                            });
                            push_rpc_op(
                                idx,
                                ApiKind::Soap,
                                op,
                                "soap-php",
                                fid,
                                call.line,
                                handler,
                                Confidence::High,
                            );
                        }
                    }
                    // setClass('Cls'): every public method of Cls becomes an
                    // operation.
                    "setClass" if soap_server_here => {
                        let Some(cls) = first_str_lit(call) else {
                            continue;
                        };
                        for f in functions.iter().filter(|f| {
                            f.containing_type.as_deref() == Some(cls.as_str())
                                && !f.name.starts_with("__")
                        }) {
                            push_rpc_op(
                                idx,
                                ApiKind::Soap,
                                f.name.clone(),
                                "soap-php",
                                fid,
                                call.line,
                                Some(f.id),
                                Confidence::High,
                            );
                        }
                    }
                    "xmlrpc_server_register_method" => {
                        // ($server, 'opName', 'handlerName')
                        if let Some(op) = first_str_lit(call) {
                            let handler = nth_str_lit(call, 1)
                                .and_then(|h| name_index.get(&h).cloned())
                                .and_then(|ids| (ids.len() == 1).then(|| ids[0]));
                            push_rpc_op(
                                idx,
                                ApiKind::XmlRpc,
                                op,
                                "xmlrpc-php",
                                fid,
                                call.line,
                                handler,
                                Confidence::High,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        Lang::Go => {
            // Generated registration: pb.RegisterGreeterServer(s, &impl{}).
            if !has(fid, &["grpc"]) {
                return;
            }
            for call in calls {
                if let Some(service) = call
                    .name
                    .strip_prefix("Register")
                    .and_then(|s| s.strip_suffix("Server"))
                {
                    if !service.is_empty() {
                        push_rpc_op(
                            idx,
                            ApiKind::Grpc,
                            service.to_string(),
                            "grpc",
                            fid,
                            call.line,
                            None,
                            Confidence::High,
                        );
                    }
                }
            }
        }
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            let grpc = has(fid, &["@grpc/grpc-js", "grpc"]);
            let jsonrpc = has(fid, &["json-rpc-2.0", "jayson"]);
            if !grpc && !jsonrpc {
                return;
            }
            for call in calls {
                // @grpc/grpc-js: server.addService(proto.Greeter.service, impl).
                if grpc && call.name == "addService" {
                    if let Some(service) = call
                        .arg_lits
                        .iter()
                        .find(|l| l.kind == LitKind::Ident && l.text.contains('.'))
                        .map(|l| grpc_service_from_ident(&l.text))
                    {
                        push_rpc_op(
                            idx,
                            ApiKind::Grpc,
                            service,
                            "grpc",
                            fid,
                            call.line,
                            None,
                            Confidence::Heuristic,
                        );
                    }
                }
                // json-rpc-2.0 / jayson: server.addMethod("name", fn).
                if jsonrpc && call.name == "addMethod" {
                    if let Some(op) = first_str_lit(call) {
                        let handler = handler_from_ident(call, func, file, functions, name_index);
                        push_rpc_op(
                            idx,
                            ApiKind::JsonRpc,
                            op,
                            "json-rpc",
                            fid,
                            call.line,
                            handler,
                            Confidence::High,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

/// Outbound RPC-style calls: SOAP clients (zeep/suds `client.service.Op()`,
/// PHP `__soapCall`), XML-RPC proxies, JSON-RPC `client.request("op")`, and
/// gRPC stub construction (`NewGreeterClient` / `GreeterStub` — service-level,
/// like the server side).
fn detect_rpc_client(
    func: &FunctionInfo,
    file: &FileInfo,
    calls: &[RawCall],
    has: &dyn Fn(u32, &[&str]) -> bool,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;
    for call in calls {
        let hit: Option<(ApiKind, String, &'static str, Confidence)> = match file.language {
            Lang::Python => {
                let recv = call.receiver.as_deref().unwrap_or("");
                if (recv.ends_with(".service") || recv == "service") && has(fid, &["zeep", "suds"])
                {
                    // zeep/suds dynamic SOAP proxy: client.service.GetUser().
                    // Receiver-shape gated -> Heuristic.
                    let lib = if has(fid, &["zeep"]) { "zeep" } else { "suds" };
                    Some((ApiKind::Soap, call.name.clone(), lib, Confidence::Heuristic))
                } else if matches!(recv, "proxy" | "rpc") && has(fid, &["xmlrpc"]) {
                    // xmlrpc.client.ServerProxy conventionally bound to
                    // `proxy`; the binding itself is not tracked.
                    Some((
                        ApiKind::XmlRpc,
                        call.name.clone(),
                        "xmlrpc-client",
                        Confidence::Heuristic,
                    ))
                } else if call.name.ends_with("Stub") && call.name.len() > 4 && has(fid, &["grpc"])
                {
                    // greeter_pb2_grpc.GreeterStub(channel) — service-level.
                    Some((
                        ApiKind::Grpc,
                        call.name.trim_end_matches("Stub").to_string(),
                        "grpc",
                        Confidence::High,
                    ))
                } else if call.name == "request" && has(fid, &["jsonrpcclient"]) {
                    first_str_lit(call)
                        .map(|op| (ApiKind::JsonRpc, op, "jsonrpcclient", Confidence::High))
                } else {
                    None
                }
            }
            Lang::Php => {
                // $client->__soapCall('GetUser', [...]): the explicit escape
                // hatch every generated/dynamic SOAP client shares.
                if call.name == "__soapCall" {
                    first_str_lit(call).map(|op| (ApiKind::Soap, op, "soap-php", Confidence::High))
                } else {
                    None
                }
            }
            Lang::Go => {
                if has(fid, &["grpc"]) {
                    call.name
                        .strip_prefix("New")
                        .and_then(|s| s.strip_suffix("Client"))
                        .filter(|s| !s.is_empty())
                        .map(|s| (ApiKind::Grpc, s.to_string(), "grpc", Confidence::High))
                } else {
                    None
                }
            }
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
                if call.name == "request"
                    && call.receiver.is_some()
                    && has(fid, &["json-rpc-2.0", "jayson"])
                {
                    first_str_lit(call)
                        .map(|op| (ApiKind::JsonRpc, op, "json-rpc", Confidence::High))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((kind, op, library, confidence)) = hit {
            let Some(norm) = normalize_path(&ensure_slash(&op)) else {
                continue;
            };
            idx.client_calls.push(ClientCall {
                id: idx.client_calls.len() as u32,
                kind,
                method: HttpMethod::Any,
                url_raw: op,
                path_norm: norm,
                library: library.to_string(),
                caller: func.id,
                file_id: fid,
                line: call.line,
                confidence,
            });
        }
    }
}

/// `proto.Greeter.service` -> `Greeter`; `GreeterGrpc.bindService` handles ->
/// last non-`service` dotted segment, `Grpc`/`Impl` suffixes stripped.
fn grpc_service_from_ident(text: &str) -> String {
    let seg = text
        .split('.')
        .rev()
        .find(|s| !matches!(*s, "service" | "Service"))
        .unwrap_or(text);
    let seg = seg.strip_prefix("new ").unwrap_or(seg).trim();
    seg.trim_end_matches("Impl")
        .trim_end_matches("Grpc")
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn push_rpc_op(
    idx: &mut EndpointIndex,
    kind: ApiKind,
    op: String,
    framework: &str,
    file_id: u32,
    line: u32,
    handler: Option<u32>,
    confidence: Confidence,
) {
    let Some(norm) = normalize_path(&ensure_slash(&op)) else {
        return;
    };
    idx.endpoints.push(Endpoint {
        id: idx.endpoints.len() as u32,
        kind,
        method: HttpMethod::Any,
        path_raw: op,
        path_norm: norm,
        framework: framework.to_string(),
        file_id,
        line,
        handler,
        confidence,
    });
}

#[allow(clippy::too_many_arguments)]
fn detect_server(
    func: &FunctionInfo,
    file: &FileInfo,
    calls: &[RawCall],
    decos: &[RawDecoration],
    has: &dyn Fn(u32, &[&str]) -> bool,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
    ctx: &FileCtx,
    project_evidence: &str,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;
    match file.language {
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            // NestJS: `@Get(':id')` method decorators + `@Controller` prefix.
            if has(fid, &["@nestjs"]) {
                for d in decos {
                    let Some(method) = HttpMethod::from_name(&d.name) else {
                        continue;
                    };
                    let sub = first_deco_str(d).unwrap_or_default();
                    let prefix = func
                        .containing_type
                        .as_ref()
                        .and_then(|t| ctx.controller_prefix.get(&(fid, t.clone())));
                    // Without a recovered @Controller prefix the emitted path
                    // may be missing its leading segment -> Heuristic.
                    let (path, conf) = match prefix {
                        Some(p) => (join_prefix(p, &sub), Confidence::High),
                        None => (ensure_slash(&sub), Confidence::Heuristic),
                    };
                    // Global `app.setGlobalPrefix('api')` join (cross-file
                    // assumption; `exclude:` options invisible) -> Heuristic.
                    let (path, conf) = match ctx.nest_prefix.clone().flatten() {
                        Some(gp) => (join_prefix(&gp, &path), Confidence::Heuristic),
                        None => (path, conf),
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method,
                        path_raw: path,
                        path_norm: norm,
                        framework: "nestjs".into(),
                        file_id: fid,
                        line: d.line,
                        handler: Some(func.id),
                        confidence: conf,
                    });
                }
            }
            // AWS CDK stacks (TypeScript flavor).
            if has(fid, CDK_EV_JS) {
                detect_cdk(func, calls, ctx, idx);
            }
            if !has(
                fid,
                &[
                    "express",
                    "fastify",
                    "@koa/router",
                    "koa-router",
                    "hono",
                    "restify",
                ],
            ) {
                return;
            }
            // Most-specific evidence first: "express" also appears inside
            // package names like "@nestjs/platform-express".
            let fw = if has(fid, &["fastify"]) {
                "fastify"
            } else if has(fid, &["@koa/router", "koa-router"]) {
                "koa"
            } else if has(fid, &["hono"]) {
                "hono"
            } else if has(fid, &["restify"]) {
                "restify"
            } else {
                "express"
            };
            // Mount prefixes (pre-pass): a same-file `app.use("/p", router)`
            // / `new Router({ prefix })` keyed by the endpoint call's
            // receiver, composed under the file's own (transitive) cross-file
            // mount prefix when both exist. A bare cross-file mount applies
            // to every endpoint declared here (over-broad when one file
            // holds several routers — hence Heuristic). `use()` ordering is
            // not modeled.
            let file_prefix = file_mount_prefix(ctx, fid).or_else(|| {
                // @fastify/autoload convention: files under a `routes/` dir
                // register at a prefix derived from their directory path
                // (routes/api/tasks/index.ts -> /api/tasks). Gated on the
                // plugin appearing in package.json (project evidence).
                (fw == "fastify" && project_evidence.contains("@fastify/autoload"))
                    .then(|| {
                        let rel = file
                            .path
                            .rfind("/routes/")
                            .map(|i| &file.path[i + 8..])
                            .or_else(|| file.path.strip_prefix("routes/"))?;
                        let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                        (!dir.is_empty()).then(|| format!("/{dir}"))
                    })
                    .flatten()
            });
            let mount_for = |recv: Option<&str>| -> Option<String> {
                let rp = recv
                    .and_then(|r| ctx.mount_recv.get(&(fid, r.to_string())))
                    .cloned()
                    .flatten();
                match (file_prefix.clone(), rp) {
                    (Some(f), Some(r)) => Some(join_prefix(&f, &r)),
                    (f, r) => f.or(r),
                }
            };
            // Express `router.route('/x').get(h).post(h)` chains: the
            // route() call defines the path; the chained verb calls arrive
            // receiver-less but share the chain's start byte (the same
            // containment trick Laravel prefix->group uses).
            let route_spans: Vec<(u32, u32, String, Option<&str>)> = calls
                .iter()
                .filter(|c| c.name == "route" && c.receiver.is_some())
                .filter_map(|c| {
                    first_path_lit(c)
                        .map(|p| (c.start_byte, c.end_byte, p, c.receiver.as_deref()))
                })
                .collect();
            for call in calls {
                let Some(method) = HttpMethod::from_name(&call.name) else {
                    continue;
                };
                if call.name == "route" || call.name == "match" {
                    // fastify.route({ method: "PUT", url: "/x" })
                    let m = str_lit_by_key(call, &["method"])
                        .and_then(|m| HttpMethod::from_name(&m))
                        .unwrap_or(HttpMethod::Any);
                    if let Some(path) = str_lit_by_key(call, &["url", "path"]) {
                        let (path, conf) = match mount_for(call.receiver.as_deref()) {
                            Some(mp) => (join_prefix(&mp, &path), Confidence::Heuristic),
                            None => (path, Confidence::High),
                        };
                        if let Some(norm) = normalize_path(&path) {
                            push_endpoint(idx, m, path, norm, fw, func, call, None, conf);
                        }
                    }
                    continue;
                }
                // Chained verb: bind to the route() span sharing this
                // chain's start byte. The span's receiver keys the mount.
                let chain = call
                    .receiver
                    .is_none()
                    .then(|| {
                        route_spans
                            .iter()
                            .find(|(s, e, _, _)| *s == call.start_byte && *e < call.end_byte)
                    })
                    .flatten();
                let (path, recv) = match chain {
                    Some((_, _, p, r)) => (p.clone(), *r),
                    None => match first_path_lit(call) {
                        Some(p) => (p, call.receiver.as_deref()),
                        None => continue,
                    },
                };
                let (path, conf) = match mount_for(recv) {
                    Some(mp) => (join_prefix(&mp, &path), Confidence::Heuristic),
                    None => (path, Confidence::High),
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let handler = handler_from_ident(call, func, file, functions, name_index);
                push_endpoint(idx, method, path, norm, fw, func, call, handler, conf);
            }
        }
        Lang::Python => {
            // AWS CDK stacks (Python flavor).
            if has(fid, CDK_EV_PY) {
                detect_cdk(func, calls, ctx, idx);
            }
            // Django URLconf: gated on the `urls.py` naming convention.
            if file.path.ends_with("urls.py") {
                let django = has(fid, &["django"]);
                for call in calls {
                    let legacy = matches!(call.name.as_str(), "url" | "re_path");
                    if call.name != "path" && !legacy {
                        continue;
                    }
                    let Some(raw) = first_str_lit(call) else {
                        continue;
                    };
                    let mut p = raw;
                    let conf = if legacy || !django {
                        Confidence::Heuristic
                    } else {
                        Confidence::High
                    };
                    if legacy {
                        // url(r'^users/$', ...): accept only when, after the
                        // ^...$ anchors, the pattern is a plain path (no
                        // regex normalization attempted).
                        p = p.trim_start_matches('^').trim_end_matches('$').to_string();
                        if p.contains(|c: char| "\\^$*+?()[]|".contains(c)) {
                            continue;
                        }
                    }
                    let path = ensure_slash(&p);
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    let handler = handler_from_ident(call, func, file, functions, name_index);
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method: HttpMethod::Any,
                        path_raw: path,
                        path_norm: norm,
                        framework: "django".into(),
                        file_id: fid,
                        line: call.line,
                        handler,
                        confidence: conf,
                    });
                }
            }
            let flask = has(fid, &["flask"]);
            let fastapi = has(fid, &["fastapi"]);
            if !flask && !fastapi {
                return;
            }
            let fw = if fastapi { "fastapi" } else { "flask" };
            for d in decos {
                let verb = d.name.rsplit('.').next().unwrap_or(&d.name);
                let is_route = verb == "route";
                let method = HttpMethod::from_name(verb);
                if !is_route && method.is_none() {
                    continue;
                }
                let Some(path) = first_deco_str(d) else {
                    continue;
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let methods: Vec<HttpMethod> = if is_route {
                    let listed: Vec<HttpMethod> = d
                        .arg_lits
                        .iter()
                        .filter(|l| l.key.as_deref() == Some("methods") && l.kind == LitKind::Str)
                        .filter_map(|l| HttpMethod::from_name(&l.text))
                        .collect();
                    if listed.is_empty() {
                        vec![HttpMethod::Get]
                    } else {
                        listed
                    }
                } else {
                    vec![method.unwrap()]
                };
                for m in methods {
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method: m,
                        path_raw: path.clone(),
                        path_norm: norm.clone(),
                        framework: fw.to_string(),
                        file_id: fid,
                        line: d.line,
                        handler: Some(func.id),
                        confidence: Confidence::High,
                    });
                }
            }
        }
        Lang::Php => {
            // Laravel: Route::get('/x', ...), Route::match([...], '/x'),
            // Route::any('/x'), Route::resource('users', ...).
            // Known limits (validated against crater-invoice/crater and the
            // laravel/laravel v8.6.12 skeleton): the RouteServiceProvider's
            // out-of-file base prefix (classically `api` for routes/api.php)
            // is invisible here, so detected paths are file-local truth.
            // Documented candidates, not implemented (no near-working shape
            // today): Lumen `$router->get(...)` + `$router->group(['prefix'
            // => ...])` (needs its own evidence gate and receiver rule),
            // CakePHP `$routes->connect(...)`, CodeIgniter `$routes->
            // get(...)`.
            let laravel_ev = has(fid, &["illuminate"]);
            let conf = if laravel_ev {
                Confidence::High
            } else {
                Confidence::Heuristic
            };
            // Route::prefix('/x')->group(fn): the chained receiver text
            // contains parens, so receiver sanitization drops it and the
            // `group` call arrives receiver-less — but the whole-chain call
            // node starts at the same byte as the inner `Route::prefix(...)`
            // call, which identifies the pairing. Calls inside the group
            // closure sit inside the group call's byte range (closures are
            // not captured as functions, so all these calls share one flat
            // list) — the same containment trick gorilla `.Methods()` uses.
            // `Route::group(['prefix' => ...], fn)` is covered too: the
            // harvester digs array-element initializers, so the prefix
            // arrives as a `prefix`-keyed Str lit on the group call itself.
            let groups: Vec<(u32, u32, String)> = calls
                .iter()
                .filter(|g| g.name == "group")
                .filter_map(|g| {
                    // Receiver `Some("Route")` is the chain-initial spelling
                    // (`Route::prefix('/x')->group(...)`); `None` is a
                    // mid-chain prefix (`Route::middleware([...])
                    // ->prefix('x')->group(...)` — crater's routes/api.php
                    // uses this throughout): the chained receiver text
                    // contains parens, so sanitization drops it, but the
                    // shared chain-start byte still pairs it with the group.
                    let chained = calls
                        .iter()
                        .find(|p| {
                            p.name == "prefix"
                                && matches!(p.receiver.as_deref(), Some("Route") | None)
                                && p.start_byte == g.start_byte
                                && p.end_byte < g.end_byte
                        })
                        .and_then(first_str_lit);
                    let arrayed = if g.receiver.as_deref() == Some("Route") {
                        str_lit_by_key(g, &["prefix"])
                    } else {
                        None
                    };
                    chained.or(arrayed).map(|p| (g.start_byte, g.end_byte, p))
                })
                .collect();
            // All enclosing group prefixes, outermost first, compounded —
            // nested `prefix()->group()` blocks join in declaration order.
            let group_prefix = |call: &RawCall| -> Option<String> {
                let mut encl: Vec<&(u32, u32, String)> = groups
                    .iter()
                    .filter(|(s, e, _)| *s < call.start_byte && *e >= call.end_byte)
                    .collect();
                if encl.is_empty() {
                    return None;
                }
                encl.sort_by_key(|g| g.0);
                Some(
                    encl.iter()
                        .fold(String::new(), |acc, (_, _, p)| join_prefix(&acc, p)),
                )
            };
            // `Route::middleware('auth')->get('/user', ...)` (the Laravel 8
            // skeleton's api.php shape): the verb call ends a chain, so its
            // receiver text contains parens and sanitization drops it — but a
            // `Route`-received call sharing the chain's start byte proves the
            // chain is a route registration (same pairing the prefix->group
            // join uses). Non-verb chain members (middleware/name/prefix)
            // fail HttpMethod::from_name below and never become endpoints.
            let chained_route = |c: &RawCall| {
                c.receiver.is_none()
                    && calls.iter().any(|p| {
                        p.receiver.as_deref() == Some("Route")
                            && p.start_byte == c.start_byte
                            && p.end_byte < c.end_byte
                    })
            };
            for call in calls {
                if call.receiver.as_deref() != Some("Route") && !chained_route(call) {
                    continue;
                }
                match call.name.as_str() {
                    "match" => {
                        // Route::match(['get', 'post'], '/x', ...): the verb
                        // array is argument 0, the path the first '/'-lit.
                        let listed: Vec<HttpMethod> = call
                            .arg_lits
                            .iter()
                            .filter(|l| l.index == 0 && l.kind == LitKind::Str && l.key.is_none())
                            .filter_map(|l| HttpMethod::from_name(&l.text))
                            .collect();
                        let Some(path) = path_shaped_lit(call) else {
                            continue;
                        };
                        let (path, conf) = match group_prefix(call) {
                            Some(gp) => (join_prefix(&gp, &path), Confidence::Heuristic),
                            None => (path, conf),
                        };
                        let Some(norm) = normalize_path(&path) else {
                            continue;
                        };
                        for m in if listed.is_empty() {
                            vec![HttpMethod::Any]
                        } else {
                            listed
                        } {
                            idx.endpoints.push(Endpoint {
                                id: idx.endpoints.len() as u32,
                                kind: ApiKind::Http,
                                method: m,
                                path_raw: path.clone(),
                                path_norm: norm.clone(),
                                framework: "laravel".into(),
                                file_id: fid,
                                line: call.line,
                                handler: None,
                                confidence: conf,
                            });
                        }
                    }
                    "resource" | "apiResource" => {
                        // Route::resource('photos', Controller::class):
                        // conventional 7-route expansion; Route::apiResource
                        // drops the HTML-form routes (create/edit — crater
                        // registers a dozen of these). Nested resources
                        // ('photos.comments') are skipped. Enclosing group
                        // prefixes ARE joined: crater's `Route::prefix('/v1')
                        // ->group(...)` wraps every resource call, and the
                        // unprefixed expansion was simply wrong against the
                        // real route list (`php artisan route:list` truth).
                        let Some(base) = first_str_lit(call) else {
                            continue;
                        };
                        let base = base.trim_matches('/').to_string();
                        if base.is_empty() || base.contains(['.', '/']) {
                            continue;
                        }
                        let base = match group_prefix(call) {
                            Some(gp) => join_prefix(&gp, &base).trim_matches('/').to_string(),
                            None => base,
                        };
                        expand_resource(
                            idx,
                            &base,
                            "{id}",
                            true,
                            call.name == "apiResource",
                            "laravel",
                            fid,
                            call.line,
                        );
                    }
                    _ => {
                        let Some(method) = HttpMethod::from_name(&call.name) else {
                            continue;
                        };
                        // Const resolution BEFORE the plain path literal: for
                        // `Route::get(BASE . '/x')` the first '/'-lit is the
                        // concat TAIL, which alone would be a wrong path.
                        // Slash-less URIs (`Route::post('login', ...)` —
                        // routine in real apps, crater included) are accepted
                        // from argument 0 only: a verb call's first argument
                        // is the URI by API contract.
                        let (path, conf) = match php_const_path(call, fid, ctx) {
                            Some(p) => (p, Confidence::Heuristic),
                            None => match call
                                .arg_lits
                                .iter()
                                .find(|l| l.index == 0 && l.key.is_none() && l.kind == LitKind::Str)
                            {
                                Some(l) => (ensure_slash(&l.text), conf),
                                None => continue,
                            },
                        };
                        let (path, conf) = match group_prefix(call) {
                            Some(gp) => (join_prefix(&gp, &path), Confidence::Heuristic),
                            None => (path, conf),
                        };
                        let Some(norm) = normalize_path(&path) else {
                            continue;
                        };
                        idx.endpoints.push(Endpoint {
                            id: idx.endpoints.len() as u32,
                            kind: ApiKind::Http,
                            method,
                            path_raw: path,
                            path_norm: norm,
                            framework: "laravel".into(),
                            file_id: fid,
                            line: call.line,
                            // Legacy string handlers 'UserController@show',
                            // Laravel 8 invokables (Controller::class) and
                            // tuples ([Controller::class, 'show']).
                            handler: laravel_string_handler(call, functions, name_index)
                                .or_else(|| {
                                    laravel_invokable_handler(call, functions, name_index)
                                })
                                .or_else(|| laravel_tuple_handler(call, functions, name_index)),
                            confidence: conf,
                        });
                    }
                }
            }
            // File evidence (imports or an implements edge) unlocks the full
            // any-`$`-receiver rule; composer.json project evidence alone
            // covers script-style files (`$app` arrives via `require`) but is
            // restricted to the `$app` receiver so Guzzle-style `$client`
            // calls can't masquerade as routes.
            let silex_file = has(fid, SILEX_EV);
            // Project evidence never overrides a file's OWN framework
            // evidence — a Slim-importing file in a repo that also depends on
            // silex/silex stays Slim.
            let silex_proj =
                !silex_file && !has(fid, &["slim"]) && project_evidence.contains("silex/silex");
            let silex = silex_file || silex_proj;
            // Slim (classic PHP micro-framework): $app->get('/x', ...).
            // Slim's own `$app->group('/p', ...)` prefixes are not joined
            // (different chain shape from Laravel's, no fixture demand yet).
            // Silex shares the call shape exactly, so its FILE evidence wins
            // (project-level silex evidence must not suppress a Slim file).
            if has(fid, &["slim"]) && !silex_file {
                for call in calls {
                    if call.receiver.as_deref() != Some("$app") {
                        continue;
                    }
                    let Some(method) = HttpMethod::from_name(&call.name) else {
                        continue;
                    };
                    let Some(path) = first_path_lit(call) else {
                        continue;
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    push_endpoint(
                        idx,
                        method,
                        path,
                        norm,
                        "slim",
                        func,
                        call,
                        None,
                        Confidence::High,
                    );
                }
            }
            // Silex (legacy Symfony micro-framework): $app->get('/x', h) with
            // string handlers ('Ctrl::method') and mounted controller
            // collections ($controllers->get(...) after $app->mount). Any
            // `$`-receiver is accepted because collections are plain locals;
            // non-$app receivers are honest Heuristic ($this excluded — that
            // shape is method calls, not routing).
            if silex {
                // Prefix composition, outermost first: the enclosing class's
                // cross-file mount ($app->mount('/v2', new Provider()))
                // wraps a same-file collection mount ($controllers->mount(
                // '/sub', $inner)) — Silex 2 nested collections. Var-to-var
                // chains deeper than one level stay uncomposed (known gap).
                let mount_for = |call: &RawCall| -> Option<String> {
                    let recv_prefix = call
                        .receiver
                        .as_deref()
                        .and_then(|r| ctx.mount_recv.get(&(fid, r.to_string())))
                        .cloned()
                        .flatten();
                    let class_prefix = func
                        .containing_type
                        .as_deref()
                        .and_then(|t| ctx.mount_class.get(t))
                        .cloned()
                        .flatten();
                    match (class_prefix, recv_prefix) {
                        (Some(c), Some(r)) => Some(join_prefix(&c, &r)),
                        (c, r) => c.or(r),
                    }
                };
                for call in calls {
                    let Some(recv) = call.receiver.as_deref() else {
                        continue;
                    };
                    if !recv.starts_with('$') || recv == "$this" {
                        continue;
                    }
                    if silex_proj && recv != "$app" {
                        continue;
                    }
                    let Some(method) = HttpMethod::from_name(&call.name) else {
                        continue;
                    };
                    // First-arg path, tolerating collection roots: `''` and
                    // `'/'` are the mount root (extremely common in provider
                    // connect() bodies). The index-0 guard keeps a handler
                    // string at index 1 from masquerading as the path.
                    let Some(path) = call
                        .arg_lits
                        .iter()
                        .find(|l| l.kind == LitKind::Str && l.key.is_none() && l.index == 0)
                        .map(|l| l.text.as_str())
                        .filter(|t| t.is_empty() || t.starts_with('/'))
                        .map(|t| if t.is_empty() { "/".to_string() } else { t.to_string() })
                    else {
                        continue;
                    };
                    let base_conf = if recv == "$app" {
                        Confidence::High
                    } else {
                        Confidence::Heuristic
                    };
                    let (path, conf) = match mount_for(call) {
                        Some(mp) => (join_prefix(&mp, &path), Confidence::Heuristic),
                        None => (path, base_conf),
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    // Silex 1/2 `->method('GET|POST')` restriction: the
                    // chained call arrives receiver-less (the chain text
                    // contains parens, so receiver sanitization drops it)
                    // but shares the route call's chain-start byte — the
                    // same containment trick Laravel prefix->group uses.
                    // A parsed restriction replaces the route's own verb
                    // (which is ANY for `match`).
                    let listed: Vec<HttpMethod> = calls
                        .iter()
                        .find(|m| {
                            m.name == "method"
                                && m.receiver.is_none()
                                && m.start_byte == call.start_byte
                                && m.end_byte > call.end_byte
                        })
                        .and_then(first_str_lit)
                        .map(|s| s.split('|').filter_map(HttpMethod::from_name).collect())
                        .unwrap_or_default();
                    let handler = class_static_string_handler(call, functions, name_index)
                        .or_else(|| silex_service_handler(call, functions, name_index))
                        .or_else(|| array_this_handler(call, func, functions, name_index));
                    for m in if listed.is_empty() {
                        vec![method]
                    } else {
                        listed
                    } {
                        push_endpoint(
                            idx,
                            m,
                            path.clone(),
                            norm.clone(),
                            "silex",
                            func,
                            call,
                            handler,
                            conf,
                        );
                    }
                }
            }
            // `symfony` covers PHP8 attributes AND legacy docblock
            // annotations (the extractor synthesizes both as decorations);
            // `sensio` covers Symfony 2/3 apps routing exclusively through
            // the SensioFrameworkExtraBundle annotations.
            if has(fid, &["symfony", "sensio"]) {
                // Class-level docblock @Route("/prefix") joined onto every
                // method route (annotation-era Symfony; validated against
                // symfony/demo v1.7.0). Honest Heuristic: the prefix rides
                // along via nearest-following-function association.
                let class_prefix = func
                    .containing_type
                    .as_ref()
                    .and_then(|t| ctx.controller_prefix.get(&(fid, t.clone())));
                for d in decos {
                    if d.name != "Route" {
                        continue;
                    }
                    let Some(path) = first_deco_str(d) else {
                        continue;
                    };
                    let (path, conf) = match class_prefix {
                        Some(p) => (join_prefix(p, &path), Confidence::Heuristic),
                        None => (path, Confidence::High),
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    // `methods={"GET","POST"}` brace lists arrive one lit per
                    // element; `methods="GET|POST"` (equally common in the
                    // Symfony 5 docs era — symfony/demo uses it throughout)
                    // arrives as ONE pipe-joined lit, so split before parsing.
                    let mut methods: Vec<HttpMethod> = d
                        .arg_lits
                        .iter()
                        .filter(|l| l.key.as_deref() == Some("methods") && l.kind == LitKind::Str)
                        .flat_map(|l| l.text.split('|'))
                        .filter_map(HttpMethod::from_name)
                        .collect();
                    // Symfony 2/3 companion annotation: @Method({"GET"}).
                    if methods.is_empty() {
                        if let Some(md) = decos.iter().find(|d2| d2.name == "Method") {
                            methods = md
                                .arg_lits
                                .iter()
                                .filter(|l| l.kind == LitKind::Str)
                                .filter_map(|l| HttpMethod::from_name(&l.text))
                                .collect();
                        }
                    }
                    for m in if methods.is_empty() {
                        vec![HttpMethod::Any]
                    } else {
                        methods
                    } {
                        idx.endpoints.push(Endpoint {
                            id: idx.endpoints.len() as u32,
                            kind: ApiKind::Http,
                            method: m,
                            path_raw: path.clone(),
                            path_norm: norm.clone(),
                            framework: "symfony".into(),
                            file_id: fid,
                            line: d.line,
                            handler: Some(func.id),
                            confidence: conf,
                        });
                    }
                }
            }
            // Framework-agnostic fallback tier, LAST so framework-specific
            // blocks keep their precise labels and this only catches what
            // they missed (unknown micro-frameworks, project router
            // wrappers, Lumen $router->get, ...). Two shapes:
            //   $recv->verb('/path', handler, ...)   — arg_count >= 2 keeps
            //     single-arg HTTP-client gets out; client-ish receiver names
            //     are excluded outright.
            //   $recv->route('GET /path', handler) / ->map('GET|POST /x', h)
            //     — Fat-Free/klein-style verb-in-string.
            // Always Heuristic, framework "php"; a same-file:line row from a
            // framework block above suppresses the generic one.
            {
                const CLIENTY: &[&str] = &["client", "http", "guzzle", "curl", "browser"];
                for call in calls {
                    let Some(recv) = call.receiver.as_deref() else {
                        continue;
                    };
                    if !recv.starts_with('$') || recv == "$this" {
                        continue;
                    }
                    let recv_lc = recv.to_ascii_lowercase();
                    if CLIENTY.iter().any(|c| recv_lc.contains(c)) {
                        continue;
                    }
                    let verb_string = matches!(call.name.as_str(), "route" | "map")
                        .then(|| first_str_lit(call))
                        .flatten()
                        .and_then(|s| {
                            let (verbs, path) = s.split_once(' ')?;
                            let parsed = verbs
                                .split('|')
                                .map(|v| HttpMethod::from_name(v.trim()))
                                .collect::<Option<Vec<HttpMethod>>>()?;
                            path.starts_with('/')
                                .then(|| (parsed, path.trim().to_string()))
                        });
                    let (methods, path) = if let Some((v, p)) = verb_string {
                        (v, p)
                    } else {
                        let Some(m) = HttpMethod::from_name(&call.name) else {
                            continue;
                        };
                        if call.arg_count < 2 {
                            continue;
                        }
                        let Some(p) = call
                            .arg_lits
                            .iter()
                            .find(|l| l.kind == LitKind::Str && l.key.is_none() && l.index == 0)
                            .filter(|l| l.text.starts_with('/'))
                            .map(|l| l.text.clone())
                        else {
                            continue;
                        };
                        (vec![m], p)
                    };
                    if idx
                        .endpoints
                        .iter()
                        .any(|e| e.file_id == fid && e.line == call.line)
                    {
                        continue;
                    }
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    let handler = class_static_string_handler(call, functions, name_index)
                        .or_else(|| silex_service_handler(call, functions, name_index))
                        .or_else(|| array_this_handler(call, func, functions, name_index))
                        .or_else(|| handler_from_ident(call, func, file, functions, name_index));
                    for m in methods {
                        push_endpoint(
                            idx,
                            m,
                            path.clone(),
                            norm.clone(),
                            "php",
                            func,
                            call,
                            handler,
                            Confidence::Heuristic,
                        );
                    }
                }
            }
        }
        Lang::Java => {
            detect_spring(func, decos, has, ctx, idx);
            detect_jaxrs(func, decos, has, ctx, idx);
        }
        Lang::Kotlin => {
            // Spring-Kotlin and JAX-RS-Kotlin share the Java annotation
            // shapes.
            detect_spring(func, decos, has, ctx, idx);
            detect_jaxrs(func, decos, has, ctx, idx);
            // Ktor: bare `get("/x") { ... }` verb calls inside routing
            // blocks. Route DSL lambdas are not captured as functions, so a
            // verb call and its surrounding `route("/api") { ... }` calls
            // share this flat call list. Lambda-suffixed calls parse as two
            // nested call_expressions and generic capture keeps only the
            // lambda-less inner span, so the Kotlin query adds an outer-span
            // capture for calls named `route` (see src/lang/kotlin.rs) —
            // byte containment against those spans recovers the (possibly
            // nested) prefix chain; each route call's short inner-span
            // duplicate encloses nothing and is harmless here. Still
            // Heuristic: prefixes built dynamically or registered from
            // another function (Route extension fns) are invisible.
            if has(fid, &["io.ktor"]) {
                let route_spans: Vec<(u32, u32, String)> = calls
                    .iter()
                    .filter(|c| c.name == "route" && c.receiver.is_none())
                    .filter_map(|c| first_str_lit(c).map(|p| (c.start_byte, c.end_byte, p)))
                    .collect();
                for call in calls {
                    if call.receiver.is_some() {
                        continue;
                    }
                    let Some(method) = HttpMethod::from_name(&call.name) else {
                        continue;
                    };
                    if method == HttpMethod::Any {
                        continue; // skip route()/match() containers
                    }
                    let mut encl: Vec<&(u32, u32, String)> = route_spans
                        .iter()
                        .filter(|(s, e, _)| *s < call.start_byte && *e >= call.end_byte)
                        .collect();
                    encl.sort_by_key(|r| r.0); // outermost first
                    let prefix = encl
                        .iter()
                        .fold(String::new(), |acc, (_, _, p)| join_prefix(&acc, p));
                    // Ktor paths need no leading slash (`route("wish") {
                    // post("make") }` == /wish/make), and a verb with no path
                    // argument at all (`get { }`) binds to the enclosing
                    // route's own path. Both shapes are only trusted INSIDE a
                    // route(...) span — a bare `get("token")` call with no
                    // route evidence around it is more likely a map lookup
                    // than a route.
                    let sub = first_str_lit(call);
                    let path = match (sub, prefix.is_empty()) {
                        (Some(s), true) => {
                            if !s.starts_with('/') {
                                continue;
                            }
                            s
                        }
                        (Some(s), false) => join_prefix(&prefix, &s),
                        (None, false) => prefix,
                        (None, true) => continue,
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method,
                        path_raw: path,
                        path_norm: norm,
                        framework: "ktor".into(),
                        file_id: fid,
                        line: call.line,
                        handler: None,
                        confidence: Confidence::Heuristic,
                    });
                }
            }
        }
        Lang::CSharp => {
            // Attribute routing [HttpGet("{id}")] + minimal APIs app.MapGet("/x", ...)
            for d in decos {
                let method = match d.name.as_str() {
                    "HttpGet" => HttpMethod::Get,
                    "HttpPost" => HttpMethod::Post,
                    "HttpPut" => HttpMethod::Put,
                    "HttpDelete" => HttpMethod::Delete,
                    "HttpPatch" => HttpMethod::Patch,
                    _ => continue,
                };
                let path = d
                    .arg_lits
                    .iter()
                    .find(|l| l.kind == LitKind::Str)
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                let Some(norm) = normalize_path(&ensure_slash(&path)) else {
                    continue;
                };
                idx.endpoints.push(Endpoint {
                    id: idx.endpoints.len() as u32,
                    kind: ApiKind::Http,
                    method,
                    path_raw: path,
                    path_norm: norm,
                    framework: "aspnet".into(),
                    file_id: fid,
                    line: d.line,
                    handler: Some(func.id),
                    confidence: Confidence::High,
                });
            }
            for call in calls {
                let method = match call.name.as_str() {
                    "MapGet" => HttpMethod::Get,
                    "MapPost" => HttpMethod::Post,
                    "MapPut" => HttpMethod::Put,
                    "MapDelete" => HttpMethod::Delete,
                    "MapPatch" => HttpMethod::Patch,
                    _ => continue,
                };
                let Some(path) = first_path_lit(call) else {
                    continue;
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                push_endpoint(
                    idx,
                    method,
                    path,
                    norm,
                    "aspnet",
                    func,
                    call,
                    None,
                    Confidence::High,
                );
            }
        }
        Lang::Go => {
            let net_http = has(fid, &["net/http"]);
            let gin = has(fid, &["gin-gonic"]);
            let echo = has(fid, &["labstack/echo"]);
            let chi = has(fid, &["go-chi"]);
            let gorilla = has(fid, &["gorilla/mux"]);
            let router = gin || echo || chi;
            if !net_http && !router && !gorilla {
                return;
            }
            let router_fw = if gin {
                "gin"
            } else if echo {
                "echo"
            } else {
                "chi"
            };
            for call in calls {
                let (mut method, is_handle) = match call.name.as_str() {
                    "HandleFunc" | "Handle" if net_http || gorilla => (HttpMethod::Any, true),
                    "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" if router => {
                        (HttpMethod::from_name(&call.name).unwrap(), false)
                    }
                    "Get" | "Post" | "Put" | "Delete" | "Patch" if router => {
                        (HttpMethod::from_name(&call.name).unwrap(), false)
                    }
                    _ => continue,
                };
                let Some(mut path) = first_str_lit(call) else {
                    continue;
                };
                let handler = handler_from_ident(call, func, file, functions, name_index);
                // gorilla/mux: r.HandleFunc("/x", h).Methods("GET", ...) —
                // the chained Methods() call is a separate RawCall whose
                // byte range encloses the HandleFunc call.
                if is_handle && gorilla && call.receiver.as_deref() != Some("http") {
                    if !path.starts_with('/') {
                        continue;
                    }
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    let listed: Vec<HttpMethod> = calls
                        .iter()
                        .find(|m| {
                            m.name == "Methods"
                                && m.start_byte <= call.start_byte
                                && m.end_byte > call.end_byte
                        })
                        .map(|m| {
                            m.arg_lits
                                .iter()
                                .filter(|l| l.kind == LitKind::Str && l.key.is_none())
                                .filter_map(|l| HttpMethod::from_name(&l.text))
                                .collect()
                        })
                        .unwrap_or_default();
                    for m in if listed.is_empty() {
                        vec![HttpMethod::Any]
                    } else {
                        listed
                    } {
                        idx.endpoints.push(Endpoint {
                            id: idx.endpoints.len() as u32,
                            kind: ApiKind::Http,
                            method: m,
                            path_raw: path.clone(),
                            path_norm: norm.clone(),
                            framework: "gorilla".into(),
                            file_id: fid,
                            line: call.line,
                            handler,
                            confidence: Confidence::High,
                        });
                    }
                    continue;
                }
                // Go 1.22 patterns: "GET /users/{id}"
                if is_handle {
                    if let Some((m, rest)) = path.split_once(' ') {
                        if let Some(parsed) = HttpMethod::from_name(m) {
                            method = parsed;
                            path = rest.to_string();
                        }
                    }
                }
                if !path.starts_with('/') {
                    continue;
                }
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let fw = if is_handle { "net/http" } else { router_fw };
                push_endpoint(
                    idx,
                    method,
                    path,
                    norm,
                    fw,
                    func,
                    call,
                    handler,
                    Confidence::High,
                );
            }
        }
        Lang::Ruby => {
            let sinatra = has(fid, &["sinatra"]);
            let rails_routes = file.path.ends_with("config/routes.rb");
            if !sinatra && !rails_routes {
                return;
            }
            for call in calls {
                if call.receiver.is_some() {
                    continue;
                }
                // Rails `resources :users` / `resource :profile` ->
                // conventional route expansion (Heuristic; `only:`/`except:`
                // options are not interpreted).
                if rails_routes && matches!(call.name.as_str(), "resources" | "resource") {
                    let plural = call.name == "resources";
                    for lit in call
                        .arg_lits
                        .iter()
                        .filter(|l| l.kind == LitKind::Str && l.key.is_none())
                    {
                        let base = lit.text.trim_start_matches(':');
                        if base.is_empty() || base.contains(['/', '.', ':']) {
                            continue;
                        }
                        expand_resource(idx, base, ":id", plural, false, "rails", fid, call.line);
                    }
                    continue;
                }
                let Some(method) = HttpMethod::from_name(&call.name) else {
                    continue;
                };
                if method == HttpMethod::Any && call.name != "match" {
                    continue;
                }
                let Some(path) = first_str_lit(call) else {
                    continue;
                };
                if !path.starts_with('/') {
                    continue;
                }
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let fw = if rails_routes { "rails" } else { "sinatra" };
                push_endpoint(
                    idx,
                    method,
                    path,
                    norm,
                    fw,
                    func,
                    call,
                    None,
                    Confidence::High,
                );
            }
        }
        Lang::Rust => {
            // actix-web: `#[get("/users/{id}")]` attribute macros captured as
            // decorations (the attribute precedes the fn item; the extractor
            // associates it via nearest-following-function).
            if has(fid, &["actix"]) {
                for d in decos {
                    let is_route = d.name == "route";
                    let method = HttpMethod::from_name(&d.name);
                    if !is_route && method.is_none_or(|m| m == HttpMethod::Any) {
                        continue;
                    }
                    let Some(path) = d
                        .arg_lits
                        .iter()
                        .find(|l| l.kind == LitKind::Str && l.text.starts_with('/'))
                        .map(|l| l.text.clone())
                    else {
                        continue;
                    };
                    let Some(norm) = normalize_path(&path) else {
                        continue;
                    };
                    // #[route("/x", method = "GET")]: token_tree args are
                    // flat, so the method is any non-path string literal.
                    let method = if is_route {
                        d.arg_lits
                            .iter()
                            .filter(|l| l.kind == LitKind::Str && !l.text.starts_with('/'))
                            .find_map(|l| HttpMethod::from_name(&l.text))
                            .unwrap_or(HttpMethod::Any)
                    } else {
                        method.unwrap()
                    };
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method,
                        path_raw: path,
                        path_norm: norm,
                        framework: "actix".into(),
                        file_id: fid,
                        line: d.line,
                        handler: Some(func.id),
                        confidence: Confidence::High,
                    });
                }
            }
            if !has(fid, &["axum"]) {
                return;
            }
            for call in calls {
                if call.name != "route" {
                    continue;
                }
                let Some(path) = first_path_lit(call) else {
                    continue;
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                // Method from the nested `get(handler)` call inside this
                // route call's byte range.
                let inner = calls.iter().find(|c| {
                    c.start_byte > call.start_byte
                        && c.end_byte <= call.end_byte
                        && VERBS.contains(&c.name.as_str())
                });
                let method = inner
                    .and_then(|c| HttpMethod::from_name(&c.name))
                    .unwrap_or(HttpMethod::Any);
                let handler =
                    inner.and_then(|c| handler_from_ident(c, func, file, functions, name_index));
                push_endpoint(
                    idx,
                    method,
                    path,
                    norm,
                    "axum",
                    func,
                    call,
                    handler,
                    Confidence::High,
                );
            }
        }
        _ => {}
    }
}

/// Spring annotations, shared between Java and Kotlin sources.
///
/// Class-level `@RequestMapping` prefixes are recovered via the ride-along
/// association set up in `src/lang/java.rs` / `src/lang/kotlin.rs` and keyed
/// by `containing_type` in the pre-pass (`FileCtx::spring_prefix`). A
/// ride-along annotation is recognized here by its line preceding the method
/// declaration (method-level annotations sit inside the declaration node) and
/// is skipped as a route of its own.
///
/// Methods of an OpenFeign `@FeignClient` interface (`FileCtx::client_iface`)
/// carry the same mapping annotations but describe OUTBOUND requests — they
/// are pushed as client calls (library "feign") instead of endpoints.
fn detect_spring(
    func: &FunctionInfo,
    decos: &[RawDecoration],
    has: &dyn Fn(u32, &[&str]) -> bool,
    ctx: &FileCtx,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;
    if !has(fid, &["org.springframework"]) {
        return;
    }
    for d in decos {
        // Class-level ride-along: a prefix, not a route (see fn doc).
        if d.line < func.start_line {
            continue;
        }
        let (method, needs_method_arg) = match d.name.as_str() {
            "GetMapping" => (HttpMethod::Get, false),
            "PostMapping" => (HttpMethod::Post, false),
            "PutMapping" => (HttpMethod::Put, false),
            "DeleteMapping" => (HttpMethod::Delete, false),
            "PatchMapping" => (HttpMethod::Patch, false),
            "RequestMapping" => (HttpMethod::Any, true),
            _ => continue,
        };
        let methods: Vec<HttpMethod> = if needs_method_arg {
            let listed: Vec<HttpMethod> = d
                .arg_lits
                .iter()
                .filter(|l| l.kind == LitKind::Ident)
                .filter_map(|l| {
                    l.text
                        .strip_prefix("RequestMethod.")
                        .and_then(HttpMethod::from_name)
                })
                .collect();
            if listed.is_empty() {
                vec![HttpMethod::Any]
            } else {
                listed
            }
        } else {
            vec![method]
        };
        let prefix = func
            .containing_type
            .as_ref()
            .and_then(|t| ctx.spring_prefix.get(&(fid, t.clone())));
        let is_client = func
            .containing_type
            .as_ref()
            .is_some_and(|t| ctx.client_iface.contains(&(fid, t.clone())));
        for path in spring_path_args(d) {
            // Joined prefixes stay Heuristic: the ride-along association can
            // mis-key when a mapped class declares no methods.
            let (path, conf) = match prefix {
                Some(p) => (join_prefix(p, &path), Confidence::Heuristic),
                None => (ensure_slash(&path), Confidence::High),
            };
            let Some(norm) = normalize_path(&path) else {
                continue;
            };
            for m in &methods {
                if is_client {
                    idx.client_calls.push(ClientCall {
                        id: idx.client_calls.len() as u32,
                        kind: ApiKind::Http,
                        method: *m,
                        url_raw: path.clone(),
                        path_norm: norm.clone(),
                        library: "feign".into(),
                        caller: func.id,
                        file_id: fid,
                        line: d.line,
                        confidence: conf,
                    });
                } else {
                    idx.endpoints.push(Endpoint {
                        id: idx.endpoints.len() as u32,
                        kind: ApiKind::Http,
                        method: *m,
                        path_raw: path.clone(),
                        path_norm: norm.clone(),
                        framework: "spring".into(),
                        file_id: fid,
                        line: d.line,
                        handler: Some(func.id),
                        confidence: conf,
                    });
                }
            }
        }
    }
}

/// JAX-RS (Quarkus / Micronaut / plain JEE) resource annotations, shared
/// between Java and Kotlin sources: a bare verb marker (`@GET`, `@POST`, ...)
/// on the method plus an optional method-level `@Path("sub")`, composed onto
/// the class-level `@Path("prefix")` recovered ride-along-style into
/// `FileCtx::jaxrs_prefix`. JAX-RS paths conventionally omit the leading
/// slash (`@Path("fruits")`, `@Path("{id}")`); `ensure_slash`/`join_prefix`
/// re-anchor them.
///
/// MicroProfile rest-client interfaces (`@RegisterRestClient`, in
/// `FileCtx::client_iface`) use the identical annotation surface for
/// OUTBOUND requests and are pushed as client calls (library "rest-client")
/// instead.
fn detect_jaxrs(
    func: &FunctionInfo,
    decos: &[RawDecoration],
    has: &dyn Fn(u32, &[&str]) -> bool,
    ctx: &FileCtx,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;
    if !has(fid, &["javax.ws.rs", "jakarta.ws.rs"]) {
        return;
    }
    // Method-level decorations only (class-level ride-alongs precede the
    // declaration and arrive via jaxrs_prefix / client_iface instead).
    let method_decos = || decos.iter().filter(|d| d.line >= func.start_line);
    let sub = method_decos()
        .find(|d| d.name == "Path")
        .map(|d| spring_path_arg(d))
        .unwrap_or_default();
    let prefix = func
        .containing_type
        .as_ref()
        .and_then(|t| ctx.jaxrs_prefix.get(&(fid, t.clone())));
    let is_client = func
        .containing_type
        .as_ref()
        .is_some_and(|t| ctx.client_iface.contains(&(fid, t.clone())));
    for d in method_decos() {
        // Uppercase gate: `from_name` is case-insensitive, but only the
        // all-caps forms are the JAX-RS verb annotations.
        if d.name.chars().any(|c| c.is_ascii_lowercase()) {
            continue;
        }
        let Some(method) = HttpMethod::from_name(&d.name) else {
            continue;
        };
        if method == HttpMethod::Any {
            continue;
        }
        // Joined prefixes stay Heuristic (ride-along mis-key caveat, see
        // detect_spring).
        let (path, conf) = match prefix {
            Some(p) => (join_prefix(p, &sub), Confidence::Heuristic),
            None => (ensure_slash(&sub), Confidence::High),
        };
        let Some(norm) = normalize_path(&path) else {
            continue;
        };
        if is_client {
            idx.client_calls.push(ClientCall {
                id: idx.client_calls.len() as u32,
                kind: ApiKind::Http,
                method,
                url_raw: path,
                path_norm: norm,
                library: "rest-client".into(),
                caller: func.id,
                file_id: fid,
                line: d.line,
                confidence: conf,
            });
        } else {
            idx.endpoints.push(Endpoint {
                id: idx.endpoints.len() as u32,
                kind: ApiKind::Http,
                method,
                path_raw: path,
                path_norm: norm,
                framework: "jaxrs".into(),
                file_id: fid,
                line: d.line,
                handler: Some(func.id),
                confidence: conf,
            });
        }
    }
}

/// Path argument of a Spring mapping annotation: first positional / `value` /
/// `path` string literal, defaulting to "/".
fn spring_path_arg(d: &RawDecoration) -> String {
    spring_path_args(d)
        .into_iter()
        .next()
        .unwrap_or_else(|| "/".to_string())
}

/// ALL path arguments of a Spring mapping annotation — `@GetMapping({"/a",
/// "/b"})` and `@RequestMapping(value = {"/a", "/b"}, ...)` declare one route
/// per array member, and the harvester surfaces each member as its own
/// Str lit (same index, same key). Defaults to a single "/".
fn spring_path_args(d: &RawDecoration) -> Vec<String> {
    let paths: Vec<String> = d
        .arg_lits
        .iter()
        .filter(|l| {
            l.kind == LitKind::Str
                && matches!(l.key.as_deref(), None | Some("value") | Some("path"))
        })
        .map(|l| l.text.clone())
        .collect();
    if paths.is_empty() {
        vec!["/".to_string()]
    } else {
        paths
    }
}

/// Prefix path from a Symfony class-level docblock `@Route` decoration. The
/// php.rs query captures the whole class docblock COMMENT as the decoration
/// name (see src/lang/php.rs), so a real class prefix arrives as a name
/// starting with `/**` containing `@Route(`; the first quoted string after
/// `@Route(` is the prefix. Method-level docblock @Route annotations are
/// synthesized with name == "Route" and never enter here.
fn php_class_docblock_route(d: &RawDecoration) -> Option<String> {
    if !d.name.starts_with("/**") {
        return None;
    }
    let rest = &d.name[d.name.find("@Route(")? + "@Route(".len()..];
    let q = rest.find(['"', '\''])?;
    let quote = rest.as_bytes()[q] as char;
    let rest = &rest[q + 1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// Path from a leading PHP const reference (`Route::get(BASE . '/x', ...)` /
/// `$client->get(BASE . '/x')`): fires only when the FIRST harvested lit of
/// argument 0 is an Ident naming a known `define()` const from the same file;
/// a following Str at the same argument index (the concatenation tail) is
/// joined on. Reversed concats (`'/x' . SUFFIX`) keep their leading Str and
/// take the plain-literal path instead.
fn php_const_path(call: &RawCall, fid: u32, ctx: &FileCtx) -> Option<String> {
    let first = call
        .arg_lits
        .iter()
        .find(|l| l.index == 0 && l.key.is_none())?;
    if first.kind != LitKind::Ident {
        return None;
    }
    let base = ctx.php_consts.get(&(fid, first.text.clone()))?;
    let tail = call
        .arg_lits
        .iter()
        .find(|l| l.index == 0 && l.key.is_none() && l.kind == LitKind::Str)
        .map(|l| l.text.as_str())
        .unwrap_or("");
    Some(if base.contains("://") {
        let t = if tail.is_empty() {
            String::new()
        } else {
            ensure_slash(tail)
        };
        format!("{}{}", base.trim_end_matches('/'), t)
    } else if tail.is_empty() {
        ensure_slash(base)
    } else {
        join_prefix(base, tail)
    })
}

/// AWS CDK app code (TypeScript and Python): API Gateway REST routes
/// (`addResource`/`addMethod`, `resourceForPath`, `addProxy`,
/// `LambdaRestApi`), HTTP API v2 `addRoutes`, Lambda function URLs, and
/// AppSync resolvers (pushed as `ApiKind::Graphql` ops so they correlate by
/// operation name). Framework "cdk" / "cdk-appsync".
///
/// Extraction constraints this is designed around (all verified empirically):
/// TS/JS new-expressions — both bare `new Ctor(...)` and member-form
/// `new ns.Ctor(...)` — arrive complete with their arguments, so
/// construction props (Lambda `handler:`, `NodejsFunction` `entry:`, `new
/// appsync.Resolver` fields, `CfnRoute` routeKey) are visible alongside the
/// METHOD calls (`addMethod`, `addRoutes`, `createResolver`,
/// `Code.fromAsset`). TS object-literal fields surface as
/// Ident-key/Str-value pairs (`str_lit_by_key` re-pairs them). Array members
/// inside those objects sit one level too deep for the harvester UNLESS the
/// array has exactly one element (single-child wrappers are unwrapped), so
/// `methods: [HttpMethod.GET]` surfaces as an Ident right after the
/// `methods` key ident while `methods: [GET, POST]` yields nothing and the
/// TS `addRoutes` row honestly widens to ANY. Python calls arrive complete
/// with kwargs, including full `methods=[HttpMethod.GET, ...]` ident lists.
fn detect_cdk(func: &FunctionInfo, calls: &[RawCall], ctx: &FileCtx, idx: &mut EndpointIndex) {
    let fid = func.file_id;
    let file_lambda = ctx.cdk_lambda.get(&fid).copied().flatten();
    // Borrowing the file's single resolved lambda as a route's handler is
    // the honest fallback for invisible dataflow; it always caps the row at
    // Heuristic (see `FileCtx::cdk_lambda`).
    let bind = |base: Confidence| -> (Option<u32>, Confidence) {
        match file_lambda {
            Some(h) => (Some(h), Confidence::Heuristic),
            None => (None, base),
        }
    };
    // Resource paths ARE trackable dataflow now: `assigned_to` names the
    // variable a `const items = api.root.addResource('items')` lands in, and
    // receivers name the variable later `items.addResource('{id}')` /
    // `items.addMethod(...)` calls go through. A byte-ordered walk therefore
    // rebuilds the resource tree exactly for the dominant real-world shape
    // (variables + root-anchored chains). `resolved` keeps every resolved
    // addResource/resourceForPath span so chained `.addMethod(...)` (same
    // chain-start byte) binds to its accumulated path. Anything that escapes
    // the walk (resources passed between functions, dynamic segments) falls
    // back to the old linear-chain heuristic, then to /{*}.
    let root_ish = |r: &str| r == "root" || r.ends_with(".root");
    let mut bound: FxHashMap<String, String> = FxHashMap::default();
    let mut resolved: Vec<(u32, u32, String)> = Vec::new();
    {
        let mut order: Vec<&RawCall> = calls
            .iter()
            .filter(|c| {
                matches!(
                    c.name.as_str(),
                    "addResource" | "add_resource" | "resourceForPath" | "resource_for_path"
                )
            })
            .collect();
        order.sort_by_key(|c| (c.start_byte, c.end_byte));
        for c in order {
            let Some(seg) = first_str_lit(c) else {
                continue;
            };
            let path = match c.name.as_str() {
                "resourceForPath" | "resource_for_path" => Some(ensure_slash(&seg)),
                _ => {
                    // Chain tails (`root.addResource('a').addResource('b')`)
                    // arrive receiver-less but share the chain's start byte
                    // with their (already resolved) predecessor.
                    let base = match c.receiver.as_deref() {
                        Some(r) if root_ish(r) => Some(String::new()),
                        Some(r) => bound.get(r).cloned(),
                        None => resolved
                            .iter()
                            .filter(|(s, e, _)| *s == c.start_byte && *e < c.end_byte)
                            .max_by_key(|(_, e, _)| *e)
                            .map(|(_, _, p)| p.clone()),
                    };
                    base.map(|b| join_prefix(&b, &seg))
                }
            };
            if let Some(path) = path {
                resolved.push((c.start_byte, c.end_byte, path.clone()));
                if let Some(v) = &c.assigned_to {
                    bound.insert(v.clone(), path);
                }
            }
        }
    }
    // Legacy fallback for unresolved receivers: distinct receivers + distinct
    // segments form at most one linear chain; the byte-ordered join of the
    // segments declared BEFORE an addMethod approximates its path.
    let mut resources: Vec<(u32, Option<&str>, String)> = calls
        .iter()
        .filter(|c| matches!(c.name.as_str(), "addResource" | "add_resource"))
        .filter_map(|c| first_str_lit(c).map(|s| (c.start_byte, c.receiver.as_deref(), s)))
        .collect();
    resources.sort_by_key(|r| r.0);
    let linear = !resources.is_empty()
        && resources
            .iter()
            .enumerate()
            .all(|(i, (_, recv, seg))| match recv {
                Some(r) => !resources[..i]
                    .iter()
                    .any(|(_, r2, s2)| *r2 == Some(*r) || s2 == seg),
                None => false,
            });
    let for_paths: Vec<(u32, u32, String)> = calls
        .iter()
        .filter(|c| matches!(c.name.as_str(), "resourceForPath" | "resource_for_path"))
        .filter_map(|c| first_str_lit(c).map(|p| (c.start_byte, c.end_byte, p)))
        .collect();
    // `proxy: false` LambdaRestApi stacks add explicit resources/methods ON
    // THE API VARIABLE; the boolean itself is unharvestable (not a string),
    // so a resource-ish call whose receiver chain starts at the API's
    // assigned variable is the honest signal to suppress the proxy-all row.
    // An unassigned LambdaRestApi (nothing to hang routes on) always keeps
    // its row.
    let explicit_routes_on = |var: &str| {
        calls.iter().any(|c| {
            matches!(
                c.name.as_str(),
                "addResource"
                    | "add_resource"
                    | "addMethod"
                    | "add_method"
                    | "resourceForPath"
                    | "resource_for_path"
            ) && c
                .receiver
                .as_deref()
                .is_some_and(|r| r == var || r.starts_with(&format!("{var}.")))
        })
    };

    for call in calls {
        match call.name.as_str() {
            "addMethod" | "add_method" => {
                let Some(method) = first_str_lit(call).and_then(|v| HttpMethod::from_name(&v))
                else {
                    continue;
                };
                // Resolution ladder: (1) chained `X.addResource('s')
                // .addMethod(...)` / `resourceForPath('/x').addMethod(...)`
                // — the inner call shares the chain's start byte and its
                // accumulated path is exact; (2) receiver is the API root;
                // (3) receiver is a tracked resource variable; then the
                // legacy fallbacks.
                let chained = resolved
                    .iter()
                    .filter(|(s, e, _)| *s == call.start_byte && *e < call.end_byte)
                    .max_by_key(|(_, e, _)| *e)
                    .map(|(_, _, p)| p.clone());
                let tracked = call.receiver.as_deref().and_then(|r| {
                    if root_ish(r) {
                        Some("/".to_string())
                    } else {
                        bound.get(r).cloned()
                    }
                });
                let (path, base_conf) = if let Some(p) = chained.or(tracked) {
                    (ensure_slash(&p), Confidence::High)
                } else if resources.is_empty() && for_paths.len() == 1 {
                    // Sole resourceForPath stored in a variable: the only
                    // plausible base, but the binding is assumed.
                    (ensure_slash(&for_paths[0].2), Confidence::Heuristic)
                } else if resources.is_empty() && for_paths.is_empty() {
                    if call.receiver.as_deref().is_some_and(|r| r.ends_with("root")) {
                        // api.root.addMethod(...): the root itself.
                        ("/".to_string(), Confidence::High)
                    } else {
                        ("/{*}".to_string(), Confidence::Heuristic)
                    }
                } else if linear {
                    let joined = resources
                        .iter()
                        .filter(|(s, _, _)| *s < call.start_byte)
                        .fold(String::new(), |acc, (_, _, seg)| join_prefix(&acc, seg));
                    if joined.is_empty() {
                        ("/{*}".to_string(), Confidence::Heuristic)
                    } else {
                        (joined, Confidence::Heuristic)
                    }
                } else {
                    ("/{*}".to_string(), Confidence::Heuristic)
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let (handler, conf) = bind(base_conf);
                push_endpoint(idx, method, path, norm, "cdk", func, call, handler, conf);
            }
            // addProxy() -> ANY {base}/{proxy+}; greedy by definition. RDS
            // `instance.addProxy('id', {...})` shares the name — its
            // positional id string is the discriminator (API GW addProxy
            // takes only an options object).
            "addProxy" | "add_proxy" => {
                if first_str_lit(call).is_some() {
                    continue; // rds.DatabaseInstance.addProxy — not a route
                }
                let base = call
                    .receiver
                    .as_deref()
                    .filter(|r| !root_ish(r))
                    .and_then(|r| bound.get(r).cloned())
                    .unwrap_or_default();
                let path = join_prefix(&base, "{proxy+}");
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let (handler, _) = bind(Confidence::Heuristic);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    path,
                    norm,
                    "cdk",
                    func,
                    call,
                    handler,
                    Confidence::Heuristic,
                );
            }
            // LambdaRestApi proxies EVERY method+path to one lambda. Both
            // Python constructions and TS new-expressions (bare and
            // `new apigateway.LambdaRestApi(...)` member form) surface; the
            // `handler:` prop is an ident (dataflow), so the handler is the
            // borrowed file-scope lambda either way. `proxy: false` stacks
            // (explicit addResource/addMethod in the same function) suppress
            // the proxy-all row — the explicit routes carry the truth.
            "LambdaRestApi" => {
                if call
                    .assigned_to
                    .as_deref()
                    .is_some_and(&explicit_routes_on)
                {
                    continue;
                }
                let (handler, conf) = bind(Confidence::High);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    "/{proxy+}".to_string(),
                    "/{*}".to_string(),
                    "cdk",
                    func,
                    call,
                    handler,
                    conf,
                );
            }
            // Explicit ProxyResource construct: `new ProxyResource(this, id,
            // { parent, anyMethod })` — a greedy {proxy+} under a parent
            // resource that arrives as an unharvestable ident, so the base
            // is unknown and the method (anyMethod / later addMethod with a
            // dynamic ident) is unknowable -> ANY /{proxy+} Heuristic.
            "ProxyResource" => {
                if !call
                    .arg_lits
                    .iter()
                    .any(|l| l.kind == LitKind::Ident && l.text == "parent")
                {
                    continue;
                }
                let (handler, _) = bind(Confidence::Heuristic);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    "/{proxy+}".to_string(),
                    "/{*}".to_string(),
                    "cdk",
                    func,
                    call,
                    handler,
                    Confidence::Heuristic,
                );
            }
            // StepFunctionsRestApi: ANY on the API root proxying to the
            // state machine (plus whatever explicit resources follow, which
            // the addResource/addMethod arms emit on their own).
            "StepFunctionsRestApi" => {
                let (handler, _) = bind(Confidence::Heuristic);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    "/".to_string(),
                    "/".to_string(),
                    "cdk",
                    func,
                    call,
                    handler,
                    Confidence::Heuristic,
                );
            }
            // HTTP API v2 with a default catch-all integration:
            // `new HttpApi(this, 'x', { defaultIntegration: ... })` -> the
            // $default route (ANY, every path). The prop value is a
            // construction (not harvestable), but the KEY ident surfacing is
            // proof enough. Python kwarg spelling included.
            "HttpApi" => {
                let has_default = call.arg_lits.iter().any(|l| {
                    l.kind == LitKind::Ident
                        && matches!(l.text.as_str(), "defaultIntegration" | "default_integration")
                });
                if !has_default {
                    continue;
                }
                let (handler, conf) = bind(Confidence::High);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    "$default".to_string(),
                    "/{*}".to_string(),
                    "cdk",
                    func,
                    call,
                    handler,
                    conf,
                );
            }
            // WebSocket API v2 L2: route options passed at construction
            // ($connect / $disconnect / $default) and `addRoute('key', ...)`
            // custom routes. Route keys are operation names, not HTTP paths;
            // the norm keeps them literal so rows stay distinct.
            "WebSocketApi" => {
                for (prop, key) in [
                    ("connectRouteOptions", "$connect"),
                    ("disconnectRouteOptions", "$disconnect"),
                    ("defaultRouteOptions", "$default"),
                    ("connect_route_options", "$connect"),
                    ("disconnect_route_options", "$disconnect"),
                    ("default_route_options", "$default"),
                ] {
                    if call.arg_lits.iter().any(|l| {
                        l.kind == LitKind::Ident
                            && l.key.is_none()
                            && l.text == prop
                            || l.key.as_deref() == Some(prop)
                    }) {
                        let (handler, _) = bind(Confidence::High);
                        push_endpoint(
                            idx,
                            HttpMethod::Any,
                            key.to_string(),
                            format!("/{}", key.to_ascii_lowercase()),
                            "cdk-websocket",
                            func,
                            call,
                            handler,
                            Confidence::High,
                        );
                    }
                }
            }
            "addRoute" | "add_route" => {
                let Some(key) = first_str_lit(call) else {
                    continue;
                };
                let (handler, conf) = bind(Confidence::High);
                let norm = format!("/{}", key.to_ascii_lowercase());
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    key,
                    norm,
                    "cdk-websocket",
                    func,
                    call,
                    handler,
                    conf,
                );
            }
            // HttpRoute L2 route keys: `HttpRouteKey.with('/books',
            // HttpMethod.GET)` — the path and method ride on the `with` call
            // itself, receiver-gated on the HttpRouteKey class.
            "with" => {
                if !call
                    .receiver
                    .as_deref()
                    .is_some_and(|r| r.ends_with("HttpRouteKey"))
                {
                    continue;
                }
                let Some(path) = first_path_lit(call) else {
                    continue;
                };
                let Some(norm) = normalize_path(&path) else {
                    continue;
                };
                let method = call
                    .arg_lits
                    .iter()
                    .filter(|l| l.kind == LitKind::Ident)
                    .find_map(|l| l.text.rsplit('.').next().and_then(HttpMethod::from_name))
                    .unwrap_or(HttpMethod::Any);
                let (handler, conf) = bind(Confidence::High);
                push_endpoint(idx, method, path, norm, "cdk", func, call, handler, conf);
            }
            // HTTP API v2: addRoutes({ path, methods, integration }).
            "addRoutes" | "add_routes" => {
                let Some(path) = str_lit_by_key(call, &["path"]) else {
                    continue;
                };
                let Some(norm) = normalize_path(&ensure_slash(&path)) else {
                    continue;
                };
                // Python kwarg: methods=[HttpMethod.GET, ...] arrives as
                // `methods`-keyed Idents. TS: only a single-member array
                // survives harvesting, unwrapped to an unkeyed Ident right
                // after the `methods` key ident (see fn doc) — consume that
                // run until a non-verb ident (the next prop key) stops it.
                // Multi-member TS arrays yield nothing -> honest ANY.
                let mut listed: Vec<HttpMethod> = call
                    .arg_lits
                    .iter()
                    .filter(|l| l.key.as_deref() == Some("methods") && l.kind == LitKind::Ident)
                    .filter_map(|l| {
                        l.text.rsplit('.').next().and_then(HttpMethod::from_name)
                    })
                    .collect();
                if listed.is_empty() {
                    if let Some(pos) = call.arg_lits.iter().position(|l| {
                        l.kind == LitKind::Ident && l.key.is_none() && l.text == "methods"
                    }) {
                        let key_index = call.arg_lits[pos].index;
                        listed = call.arg_lits[pos + 1..]
                            .iter()
                            .take_while(|l| {
                                l.index == key_index
                                    && l.key.is_none()
                                    && l.kind == LitKind::Ident
                            })
                            .map_while(|l| {
                                l.text.rsplit('.').next().and_then(HttpMethod::from_name)
                            })
                            .collect();
                    }
                }
                let (handler, conf) = bind(Confidence::High);
                for m in if listed.is_empty() {
                    vec![HttpMethod::Any]
                } else {
                    listed
                } {
                    push_endpoint(
                        idx,
                        m,
                        ensure_slash(&path),
                        norm.clone(),
                        "cdk",
                        func,
                        call,
                        handler,
                        conf,
                    );
                }
            }
            // Lambda function URL: one HTTPS entry point, any method/path.
            "addFunctionUrl" | "add_function_url" => {
                let (handler, _) = bind(Confidence::Heuristic);
                push_endpoint(
                    idx,
                    HttpMethod::Any,
                    "/{*}".to_string(),
                    "/{*}".to_string(),
                    "cdk",
                    func,
                    call,
                    handler,
                    Confidence::Heuristic,
                );
            }
            // L1 escape hatch (CDK v1-era code): CfnRoute carries a literal
            // "VERB /path" route key. Python constructions surface with
            // kwargs; TS `new apigwv2.CfnRoute(...)` member-form
            // constructions surface with their props object (routeKey as an
            // Ident-key/Str-value window). The Target integration chain is
            // invisible either way, so the handler is the usual borrowed
            // file-scope lambda.
            "CfnRoute" => {
                let Some(rk) = str_lit_by_key(call, &["route_key", "routeKey"]) else {
                    continue;
                };
                let rk = rk.trim().to_string();
                // Spaceless route keys are WebSocket routes ($connect /
                // $disconnect / $default / custom actions): operation names,
                // kept literal in the norm like the L2 WebSocketApi rows.
                let (method, path, norm, fw) = match rk.split_once(' ') {
                    None => {
                        let norm = format!("/{}", rk.to_ascii_lowercase());
                        (HttpMethod::Any, rk, norm, "cdk-websocket")
                    }
                    Some((m, p)) => {
                        let Some(method) = HttpMethod::from_name(m) else {
                            continue;
                        };
                        let path = ensure_slash(p.trim());
                        let Some(norm) = normalize_path(&path) else {
                            continue;
                        };
                        (method, path, norm, "cdk")
                    }
                };
                let (handler, conf) = bind(Confidence::High);
                push_endpoint(idx, method, path, norm, fw, func, call, handler, conf);
            }
            // AppSync resolvers: ds.createResolver('id', { typeName,
            // fieldName }) / options-only arity / Python create_resolver
            // kwargs / `appsync.Resolver(...)` constructions in BOTH
            // languages (TS `new appsync.Resolver` props arrive as
            // Ident-key/Str-value windows) / Python L1
            // `appsync.CfnResolver(type_name=, field_name=)`. The op is
            // "Type.field", correlating by name on the RPC branch.
            "createResolver" | "create_resolver" | "Resolver" | "CfnResolver" => {
                let t = str_lit_by_key(call, &["typeName", "type_name"]);
                let f = str_lit_by_key(call, &["fieldName", "field_name"]);
                if let (Some(t), Some(f)) = (t, f) {
                    let (handler, conf) = bind(Confidence::High);
                    push_rpc_op(
                        idx,
                        ApiKind::Graphql,
                        format!("{t}.{f}"),
                        "cdk-appsync",
                        fid,
                        call.line,
                        handler,
                        conf,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Pre-pass: harvest one file-slice's CDK Lambda declarations and try to
/// resolve each to an indexed handler function. Python constructions arrive
/// with their kwargs, so `handler="app.lambda_handler"` +
/// `Code.from_asset("src")` resolve exactly. TS/JS constructions now arrive
/// too (member-form new-expressions carry arguments): `new lambda.Function`
/// resolves its `handler:` prop against the byte-contained
/// `Code.fromAsset(...)` dir, `new NodejsFunction({ entry })` treats entry
/// as the handler source file itself, and `new PythonFunction` mirrors the
/// Python arm. A standalone `Code.fromAsset` (asset stored in a variable)
/// still counts as a declaration marker with the `index.handler` default —
/// unless it is the file's sole standalone asset AND exactly one Function
/// construction lacked a contained asset, in which case the two are the same
/// lambda and merge. All of this stays Heuristic at the routes via the
/// single-lambda borrow in `detect_cdk`.
fn collect_cdk_lambdas(
    file: &FileInfo,
    files: &[FileInfo],
    calls: &[RawCall],
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
    out: &mut Vec<Option<u32>>,
) {
    let dir = file.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    match file.language {
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            // lambda.Code.fromAsset('dir') — receiver keeps the `.Code`
            // tail, distinguishing it from other fromAsset-ish APIs
            // (appsync `Schema.fromAsset`). DockerImageFunction uses
            // fromImageAsset and is skipped (no source handler to resolve).
            let assets: Vec<&RawCall> = calls
                .iter()
                .filter(|c| {
                    c.name == "fromAsset"
                        && c.receiver
                            .as_deref()
                            .is_some_and(|r| r == "Code" || r.ends_with(".Code"))
                })
                .collect();
            let resolve_js = |asset: &str, stem: &str, export: &str| {
                ["ts", "tsx", "js", "mjs", "cjs"].iter().find_map(|ext| {
                    cdk_find_file(files, dir, &format!("{asset}/{stem}.{ext}"))
                        .and_then(|t| cdk_fn_in_file(functions, name_index, t, export))
                })
            };
            // Constructions arrive with their props object (Ident-key /
            // Str-value windows). Spans recorded so a fromAsset nested in a
            // construction is not double-counted as a second declaration.
            let mut spans: Vec<(u32, u32)> = Vec::new();
            // handler strings of `new lambda.Function` constructions whose
            // `code:` asset was NOT byte-contained (stored in a variable):
            // deferred so the file's sole standalone fromAsset can be
            // borrowed as their asset below.
            let mut deferred: Vec<String> = Vec::new();
            for call in calls {
                match call.name.as_str() {
                    // new lambda.Function(this, 'X', { handler:
                    // 'index.handler', code: Code.fromAsset('dir') }): the
                    // handler prop gates (RestApi/HttpApi/JS `new Function`
                    // lack it); asset dir from the contained fromAsset.
                    "Function" => {
                        let Some(handler) = str_lit_by_key(call, &["handler"]) else {
                            continue; // not a Lambda construction
                        };
                        spans.push((call.start_byte, call.end_byte));
                        let contained = assets
                            .iter()
                            .find(|a| {
                                a.start_byte >= call.start_byte && a.end_byte <= call.end_byte
                            })
                            .and_then(|a| first_str_lit(a));
                        match contained {
                            Some(asset) => out.push(handler.rsplit_once('.').and_then(
                                |(stem, export)| resolve_js(&asset, stem, export),
                            )),
                            None => deferred.push(handler),
                        }
                    }
                    // new NodejsFunction({ entry, handler }): entry IS the
                    // handler source file; `handler:` names the export (CDK
                    // default "handler"). A missing entry means esbuild
                    // infers it from the DEFINING file's name — not modeled,
                    // recorded as an unresolved declaration.
                    "NodejsFunction" => {
                        spans.push((call.start_byte, call.end_byte));
                        let resolved = str_lit_by_key(call, &["entry"]).and_then(|entry| {
                            let export = str_lit_by_key(call, &["handler"])
                                .unwrap_or_else(|| "handler".to_string());
                            cdk_find_file(files, dir, &entry)
                                .and_then(|t| cdk_fn_in_file(functions, name_index, t, &export))
                        });
                        out.push(resolved);
                    }
                    // TS-declared PythonFunction: same defaults as the
                    // Python arm (index="index.py", handler="handler").
                    "PythonFunction" => {
                        spans.push((call.start_byte, call.end_byte));
                        let resolved = str_lit_by_key(call, &["entry"]).and_then(|entry| {
                            let index = str_lit_by_key(call, &["index"])
                                .unwrap_or_else(|| "index.py".to_string());
                            let export = str_lit_by_key(call, &["handler"])
                                .unwrap_or_else(|| "handler".to_string());
                            let t = cdk_find_file(files, dir, &format!("{entry}/{index}"))?;
                            cdk_fn_in_file(functions, name_index, t, &export)
                        });
                        out.push(resolved);
                    }
                    _ => {}
                }
            }
            // Standalone fromAsset calls (outside every construction span):
            // `const code = lambda.Code.fromAsset('dir')` wiring.
            let standalone: Vec<&&RawCall> = assets
                .iter()
                .filter(|a| {
                    !spans
                        .iter()
                        .any(|(s, e)| a.start_byte >= *s && a.end_byte <= *e)
                })
                .collect();
            if deferred.len() == 1 && standalone.len() == 1 {
                // Sole asset-less Function + sole standalone asset: the same
                // lambda seen from both ends — one declaration.
                let resolved = first_str_lit(standalone[0]).and_then(|asset| {
                    deferred[0]
                        .rsplit_once('.')
                        .and_then(|(stem, export)| resolve_js(&asset, stem, export))
                });
                out.push(resolved);
            } else {
                // Ambiguous pairings degrade to independent declarations:
                // asset-less constructions stay unresolved, standalone assets
                // fall back to Lambda's index.handler default convention.
                for _ in &deferred {
                    out.push(None);
                }
                for a in &standalone {
                    out.push(
                        first_str_lit(a).and_then(|asset| resolve_js(&asset, "index", "handler")),
                    );
                }
            }
        }
        Lang::Python => {
            for call in calls {
                match call.name.as_str() {
                    // _lambda.Function(..., handler="app.lambda_handler",
                    // code=_lambda.Code.from_asset("src")): the asset dir is
                    // available both as the byte-contained from_asset call and
                    // as a `code`-keyed Str harvested one level down.
                    "Function" => {
                        let Some(handler) = str_lit_by_key(call, &["handler"]) else {
                            continue; // not a Lambda construction
                        };
                        let asset = calls
                            .iter()
                            .find(|c| {
                                c.name == "from_asset"
                                    && c.start_byte >= call.start_byte
                                    && c.end_byte <= call.end_byte
                            })
                            .and_then(first_str_lit)
                            .or_else(|| str_lit_by_key(call, &["code"]));
                        let resolved = asset.and_then(|asset| {
                            let (stem, export) = handler.rsplit_once('.')?;
                            let t = cdk_find_file(files, dir, &format!("{asset}/{stem}.py"))?;
                            cdk_fn_in_file(functions, name_index, t, export)
                        });
                        out.push(resolved);
                    }
                    // L1 CfnFunction (v1-era escape hatch): template-shaped
                    // props carry no local asset dir, so the handler stem is
                    // tried against the indexed tree directly (the
                    // path-suffix fallback in cdk_find_file does the real
                    // work). S3/inline-coded functions fail to resolve and
                    // record an unresolved declaration like any other lambda.
                    "CfnFunction" => {
                        let resolved = str_lit_by_key(call, &["handler"]).and_then(|handler| {
                            let (stem, export) = handler.rsplit_once('.')?;
                            ["py", "js", "ts", "mjs", "cjs"].iter().find_map(|ext| {
                                cdk_find_file(files, dir, &format!("{stem}.{ext}")).and_then(|t| {
                                    cdk_fn_in_file(functions, name_index, t, export)
                                })
                            })
                        });
                        out.push(resolved);
                    }
                    // PythonFunction(entry=dir, index="app.py",
                    // handler="fn") with documented CDK defaults.
                    "PythonFunction" => {
                        let resolved = str_lit_by_key(call, &["entry"]).and_then(|entry| {
                            let index = str_lit_by_key(call, &["index"])
                                .unwrap_or_else(|| "index.py".to_string());
                            let export = str_lit_by_key(call, &["handler"])
                                .unwrap_or_else(|| "handler".to_string());
                            let t = cdk_find_file(files, dir, &format!("{entry}/{index}"))?;
                            cdk_fn_in_file(functions, name_index, t, &export)
                        });
                        out.push(resolved);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Resolve a CDK asset-relative source path against the indexed file set.
/// Asset dirs are relative to the CDK app root — usually the CDK file's own
/// directory or the repo root — so try both (exact), then fall back to a
/// unique path-suffix match.
fn cdk_find_file(files: &[FileInfo], cdk_dir: &str, rel: &str) -> Option<u32> {
    let joined = if cdk_dir.is_empty() {
        rel.to_string()
    } else {
        format!("{cdk_dir}/{rel}")
    };
    for cand in [joined.as_str(), rel] {
        if let Some(f) = files.iter().find(|f| f.path == cand) {
            return Some(f.id);
        }
    }
    let sfx = format!("/{rel}");
    let hits: Vec<u32> = files
        .iter()
        .filter(|f| f.path.ends_with(&sfx))
        .map(|f| f.id)
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Unique non-toplevel function named `name` inside `file_id`.
fn cdk_fn_in_file(
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
    file_id: u32,
    name: &str,
) -> Option<u32> {
    let hits: Vec<u32> = name_index
        .get(name)?
        .iter()
        .copied()
        .filter(|&id| {
            functions[id as usize].file_id == file_id && !functions[id as usize].is_toplevel
        })
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

#[allow(clippy::too_many_arguments)]
fn detect_client(
    func: &FunctionInfo,
    file: &FileInfo,
    calls: &[RawCall],
    decos: &[RawDecoration],
    has: &dyn Fn(u32, &[&str]) -> bool,
    ctx: &FileCtx,
    idx: &mut EndpointIndex,
) {
    let fid = func.file_id;

    // Retrofit interface annotations: @GET("/users/{id}") on the method.
    if matches!(file.language, Lang::Java | Lang::Kotlin) && has(fid, &["retrofit"]) {
        for d in decos {
            let method = match d.name.as_str() {
                "GET" => HttpMethod::Get,
                "POST" => HttpMethod::Post,
                "PUT" => HttpMethod::Put,
                "DELETE" => HttpMethod::Delete,
                "PATCH" => HttpMethod::Patch,
                "HEAD" => HttpMethod::Head,
                "OPTIONS" => HttpMethod::Options,
                _ => continue,
            };
            let Some(url) = first_deco_str(d) else {
                continue;
            };
            let url = ensure_slash(&url);
            if let Some(norm) = normalize_path(&url) {
                idx.client_calls.push(ClientCall {
                    id: idx.client_calls.len() as u32,
                    kind: ApiKind::Http,
                    method,
                    url_raw: url,
                    path_norm: norm,
                    library: "retrofit".into(),
                    caller: func.id,
                    file_id: fid,
                    line: d.line,
                    confidence: Confidence::High,
                });
            }
        }
    }

    for call in calls {
        let hit: Option<(HttpMethod, String, &'static str, Confidence)> = match file.language {
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
                if call.name == "fetch" && call.receiver.is_none() {
                    first_url_lit(call).map(|url| {
                        let m = str_lit_by_key(call, &["method"])
                            .and_then(|m| HttpMethod::from_name(&m))
                            .unwrap_or(HttpMethod::Get);
                        (m, url, "fetch", Confidence::High)
                    })
                } else if call.receiver.as_deref() == Some("axios") && has(fid, &["axios"]) {
                    let m = HttpMethod::from_name(&call.name)
                        .or_else(|| (call.name == "request").then_some(HttpMethod::Any));
                    m.and_then(|m| first_url_lit(call).map(|u| (m, u, "axios", Confidence::High)))
                } else if call.name == "open"
                    && call.receiver.as_deref().is_some_and(|r| {
                        let rl = r.to_ascii_lowercase();
                        rl.contains("xhr") || rl.contains("req")
                    })
                {
                    // XMLHttpRequest#open(method, url): a browser global, so
                    // no import to gate on — the receiver name is the only
                    // evidence. Heuristic.
                    first_str_lit(call)
                        .and_then(|m| HttpMethod::from_name(&m))
                        .and_then(|m| {
                            nth_str_lit(call, 1)
                                .filter(|u| url_shaped(u))
                                .map(|u| (m, u, "xhr", Confidence::Heuristic))
                        })
                } else if matches!(call.receiver.as_deref(), Some("$") | Some("jQuery")) {
                    // jQuery is usually a <script> global: receiver-gated
                    // only, Heuristic. Legacy $.ajax uses `type:` for the
                    // method.
                    match call.name.as_str() {
                        "ajax" => str_lit_by_key(call, &["url"]).map(|u| {
                            let m = str_lit_by_key(call, &["method", "type"])
                                .and_then(|m| HttpMethod::from_name(&m))
                                .unwrap_or(HttpMethod::Get);
                            (m, u, "jquery", Confidence::Heuristic)
                        }),
                        "get" | "post" => HttpMethod::from_name(&call.name).and_then(|m| {
                            first_url_lit(call).map(|u| (m, u, "jquery", Confidence::Heuristic))
                        }),
                        "getJSON" => first_url_lit(call)
                            .map(|u| (HttpMethod::Get, u, "jquery", Confidence::Heuristic)),
                        _ => None,
                    }
                } else if let Some(lib) = ["superagent", "got", "ky"].iter().find(|lib| {
                    match call.receiver.as_deref() {
                        Some(r) => js_lib_recv(file, lib, r),
                        // Direct-call form: got("/x", { method: "POST" }).
                        None => js_lib_recv(file, lib, &call.name),
                    }
                }) {
                    if call.receiver.is_some() {
                        HttpMethod::from_name(&call.name).and_then(|m| {
                            first_url_lit(call).map(|u| (m, u, *lib, Confidence::High))
                        })
                    } else {
                        let m = str_lit_by_key(call, &["method"])
                            .and_then(|m| HttpMethod::from_name(&m))
                            .unwrap_or(HttpMethod::Get);
                        first_url_lit(call).map(|u| (m, u, *lib, Confidence::High))
                    }
                } else if let (Some(base), Some(recv)) =
                    (ctx.axios_base.get(&fid), call.receiver.as_deref())
                {
                    // axios.create({ baseURL }) instances: the variable the
                    // instance was assigned to is not tracked, so the base is
                    // applied file-wide to axios-ish receiver names
                    // (api/client/http/instance/axios). Heuristic by design.
                    let rl = recv.to_ascii_lowercase();
                    let axios_ish = ["api", "client", "http", "axios", "instance"]
                        .iter()
                        .any(|n| rl.contains(n));
                    if axios_ish {
                        HttpMethod::from_name(&call.name).and_then(|m| {
                            first_path_lit(call).map(|p| {
                                let joined = format!("{}{}", base.trim_end_matches('/'), p);
                                (m, joined, "axios", Confidence::Heuristic)
                            })
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Lang::Python => {
                let recv = call.receiver.as_deref();
                if recv == Some("requests") && has(fid, &["requests"]) {
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        first_url_lit(call).map(|u| (m, u, "requests", Confidence::High))
                    })
                } else if recv == Some("httpx") && has(fid, &["httpx"]) {
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        first_url_lit(call).map(|u| (m, u, "httpx", Confidence::High))
                    })
                } else if has(fid, &["aiohttp"])
                    && recv.is_some_and(|r| r.to_ascii_lowercase().ends_with("session"))
                {
                    // aiohttp: session.get("/x") — receiver-shape gated.
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        first_url_lit(call).map(|u| (m, u, "aiohttp", Confidence::Heuristic))
                    })
                } else {
                    None
                }
            }
            Lang::Php => {
                // PHP curl_* is skipped on purpose: the URL arrives via
                // curl_setopt(CURLOPT_URL, ...) on an opaque handle.
                if has(fid, &["guzzlehttp"]) && call.receiver.is_some() {
                    if call.name == "request" {
                        let m = first_str_lit(call)
                            .and_then(|s| HttpMethod::from_name(&s))
                            .unwrap_or(HttpMethod::Any);
                        nth_str_lit(call, 1)
                            .filter(|u| url_shaped(u))
                            .map(|u| (m, u, "guzzle", Confidence::High))
                    } else {
                        // Const-first, like the Laravel branch: for
                        // `$client->get(BASE . '/x')` the bare '/x' tail
                        // would otherwise pass as the whole URL.
                        HttpMethod::from_name(&call.name).and_then(|m| {
                            php_const_path(call, fid, ctx)
                                .map(|u| (m, u, "guzzle", Confidence::Heuristic))
                                .or_else(|| {
                                    first_url_lit(call).map(|u| (m, u, "guzzle", Confidence::High))
                                })
                        })
                    }
                } else {
                    None
                }
            }
            Lang::Go => {
                if call.receiver.as_deref() == Some("http") && has(fid, &["net/http"]) {
                    if call.name == "NewRequest" {
                        let m = first_str_lit(call)
                            .and_then(|s| HttpMethod::from_name(&s))
                            .unwrap_or(HttpMethod::Any);
                        nth_str_lit(call, 1)
                            .filter(|u| url_shaped(u))
                            .map(|u| (m, u, "net/http", Confidence::High))
                    } else {
                        HttpMethod::from_name(&call.name).and_then(|m| {
                            first_url_lit(call).map(|u| (m, u, "net/http", Confidence::High))
                        })
                    }
                } else {
                    None
                }
            }
            Lang::CSharp => {
                let m = match call.name.as_str() {
                    "GetAsync" | "GetStringAsync" | "GetFromJsonAsync" => Some(HttpMethod::Get),
                    "PostAsync" | "PostAsJsonAsync" => Some(HttpMethod::Post),
                    "PutAsync" | "PutAsJsonAsync" => Some(HttpMethod::Put),
                    "DeleteAsync" => Some(HttpMethod::Delete),
                    "PatchAsync" => Some(HttpMethod::Patch),
                    _ => None,
                };
                m.and_then(|m| first_url_lit(call).map(|u| (m, u, "httpclient", Confidence::High)))
            }
            Lang::Ruby => {
                let recv = call.receiver.as_deref();
                if recv == Some("HTTParty") && has(fid, &["httparty"]) {
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        first_url_lit(call).map(|u| (m, u, "httparty", Confidence::High))
                    })
                } else if recv == Some("Faraday") && has(fid, &["faraday"]) {
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        first_url_lit(call).map(|u| (m, u, "faraday", Confidence::High))
                    })
                } else if recv == Some("Net::HTTP") && has(fid, &["net/http"]) {
                    // Net::HTTP.get(...): only when a URL-shaped string
                    // literal is visible (URI("https://...") is harvested one
                    // level down); opaque uri variables are skipped.
                    HttpMethod::from_name(&call.name).and_then(|m| {
                        any_url_lit(call).map(|u| (m, u, "net-http-rb", Confidence::High))
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((method, url, library, confidence)) = hit {
            if let Some(norm) = normalize_path(&url) {
                idx.client_calls.push(ClientCall {
                    id: idx.client_calls.len() as u32,
                    kind: ApiKind::Http,
                    method,
                    url_raw: url,
                    path_norm: norm,
                    library: library.to_string(),
                    caller: func.id,
                    file_id: fid,
                    line: call.line,
                    confidence,
                });
            }
        }
    }
}

/// Is `recv` a plausible handle on the JS package `lib`? True when an import
/// of that exact package (or a subpath) binds `recv`, or `recv` is the
/// package's conventional name and the package is imported.
fn js_lib_recv(file: &FileInfo, lib: &str, recv: &str) -> bool {
    file.imports.iter().any(|i| {
        let p = i.path.to_ascii_lowercase();
        (p == lib || p.starts_with(&format!("{lib}/")))
            && (recv == lib || i.names.iter().any(|n| n == recv))
    })
}

#[allow(clippy::too_many_arguments)]
fn push_endpoint(
    idx: &mut EndpointIndex,
    method: HttpMethod,
    path_raw: String,
    path_norm: String,
    framework: &str,
    func: &FunctionInfo,
    call: &RawCall,
    handler: Option<u32>,
    confidence: Confidence,
) {
    idx.endpoints.push(Endpoint {
        id: idx.endpoints.len() as u32,
        kind: ApiKind::Http,
        method,
        path_raw,
        path_norm,
        framework: framework.to_string(),
        file_id: func.file_id,
        line: call.line,
        handler,
        confidence,
    });
}

/// Conventional REST resource expansion (Rails `resources`, Laravel
/// `Route::resource` / `Route::apiResource`). Always Heuristic: the routes
/// are implied by convention, not written in the source. `api` drops the
/// HTML-form routes (new/create-form, edit) the way Laravel's apiResource
/// does.
fn expand_resource(
    idx: &mut EndpointIndex,
    base: &str,
    param: &str,
    plural: bool,
    api: bool,
    framework: &str,
    file_id: u32,
    line: u32,
) {
    let root = format!("/{base}");
    let routes: Vec<(HttpMethod, String)> = if plural {
        let mut r = vec![
            (HttpMethod::Get, root.clone()), // index
            (HttpMethod::Post, root.clone()), // create/store
            (HttpMethod::Get, format!("{root}/{param}")), // show
            (HttpMethod::Patch, format!("{root}/{param}")), // update
            (HttpMethod::Delete, format!("{root}/{param}")), // destroy
        ];
        if !api {
            r.push((HttpMethod::Get, format!("{root}/new"))); // new
            r.push((HttpMethod::Get, format!("{root}/{param}/edit"))); // edit
        }
        r
    } else {
        vec![
            (HttpMethod::Get, format!("{root}/new")),
            (HttpMethod::Post, root.clone()),
            (HttpMethod::Get, root.clone()),
            (HttpMethod::Get, format!("{root}/edit")),
            (HttpMethod::Patch, root.clone()),
            (HttpMethod::Delete, root.clone()),
        ]
    };
    for (method, path) in routes {
        let Some(norm) = normalize_path(&path) else {
            continue;
        };
        idx.endpoints.push(Endpoint {
            id: idx.endpoints.len() as u32,
            kind: ApiKind::Http,
            method,
            path_raw: path,
            path_norm: norm,
            framework: framework.to_string(),
            file_id,
            line,
            handler: None,
            confidence: Confidence::Heuristic,
        });
    }
}

/// Resolve a handler passed by name (`app.get("/x", listUsers)`): identifier
/// argument matched against functions in the same file.
/// Pre-5.3 Laravel string handlers: `Route::get('/x', 'UserController@show')`
/// — resolve the method against functions whose containing type matches.
fn laravel_string_handler(
    call: &RawCall,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let lit = call
        .arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none() && l.text.contains('@'))?;
    let (ctrl, method) = lit.text.split_once('@')?;
    if ctrl.is_empty() || method.is_empty() || ctrl.contains('/') {
        return None;
    }
    let hits: Vec<u32> = name_index
        .get(method)?
        .iter()
        .copied()
        .filter(|&id| functions[id as usize].containing_type.as_deref() == Some(ctrl))
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Laravel single-action controllers: `Route::get('/x', VersionController::
/// class)` — the bare `::class` const arrives as two Idents sharing an
/// argument index ("VersionController", "class"; probe-verified against
/// tests/fixtures/probe shapes). Resolve to the class's `__invoke`.
fn laravel_invokable_handler(
    call: &RawCall,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let class = call.arg_lits.windows(2).find_map(|w| {
        (w[0].kind == LitKind::Ident
            && w[0].key.is_none()
            && w[0].index > 0
            && w[0].text.chars().next().is_some_and(|c| c.is_uppercase())
            && w[1].kind == LitKind::Ident
            && w[1].text == "class"
            && w[1].index == w[0].index)
            .then(|| w[0].text.clone())
    })?;
    let hits: Vec<u32> = name_index
        .get("__invoke")?
        .iter()
        .copied()
        .filter(|&id| functions[id as usize].containing_type.as_deref() == Some(class.as_str()))
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Laravel 8 tuple handlers `[UserController::class, 'show']`: the class
/// const sits one level below harvest depth inside the array (probe-verified
/// — only the method-name string survives), so the class is unknowable here.
/// Resolve by PROJECT-UNIQUE method name among class methods; anything
/// ambiguous stays unresolved.
fn laravel_tuple_handler(
    call: &RawCall,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let lit = call.arg_lits.iter().find(|l| {
        l.kind == LitKind::Str
            && l.key.is_none()
            && l.index > 0
            && !l.text.is_empty()
            && !l.text.contains(['@', ':', '/', '.', ' '])
    })?;
    let hits: Vec<u32> = name_index
        .get(&lit.text)?
        .iter()
        .copied()
        .filter(|&id| functions[id as usize].containing_type.is_some())
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Silex/Symfony callable strings: `'MyController::indexAction'` — resolve
/// the method against functions whose containing type matches. Cross-file
/// like `laravel_string_handler`; the class is reduced to its simple name
/// because `containing_type` never carries a namespace.
fn class_static_string_handler(
    call: &RawCall,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let lit = call
        .arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none() && l.text.contains("::"))?;
    let (ctrl, method) = lit.text.rsplit_once("::")?;
    let ctrl = ctrl.rsplit('\\').next().unwrap_or(ctrl);
    if ctrl.is_empty() || method.is_empty() || ctrl.contains('/') {
        return None;
    }
    let hits: Vec<u32> = name_index
        .get(method)?
        .iter()
        .copied()
        .filter(|&id| functions[id as usize].containing_type.as_deref() == Some(ctrl))
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Silex service-controller strings: `'widget.controller:index'` — a service
/// id and a method, single colon. The service id names a container entry, not
/// a class, so the method resolves by global unique name only (non-toplevel).
fn silex_service_handler(
    call: &RawCall,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let lit = call.arg_lits.iter().find(|l| {
        l.kind == LitKind::Str
            && l.key.is_none()
            && l.index > 0
            && l.text.contains(':')
            && !l.text.contains("::")
            && !l.text.starts_with('/')
    })?;
    let (_, method) = lit.text.rsplit_once(':')?;
    if method.is_empty() {
        return None;
    }
    let hits: Vec<u32> = name_index
        .get(method)?
        .iter()
        .copied()
        .filter(|&id| !functions[id as usize].is_toplevel)
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Array-callable handlers: `[$this, 'homepage']` — the depth-2 harvest
/// surfaces the pair as an Ident "this" and a Str sharing an index; the
/// method lives on the registering function's own class.
fn array_this_handler(
    call: &RawCall,
    func: &FunctionInfo,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    let owner = func.containing_type.as_deref()?;
    let this_idx = call
        .arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Ident && l.key.is_none() && l.text == "this")
        .map(|l| l.index)?;
    let method = call.arg_lits.iter().find(|l| {
        l.kind == LitKind::Str
            && l.key.is_none()
            && l.index == this_idx
            && !l.text.contains([':', '/'])
            && !l.text.is_empty()
    })?;
    let hits: Vec<u32> = name_index
        .get(&method.text)?
        .iter()
        .copied()
        .filter(|&id| functions[id as usize].containing_type.as_deref() == Some(owner))
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

fn handler_from_ident(
    call: &RawCall,
    func: &FunctionInfo,
    file: &FileInfo,
    functions: &[FunctionInfo],
    name_index: &FxHashMap<String, Vec<u32>>,
) -> Option<u32> {
    // Unique non-toplevel function named `name` declared in `fid`.
    let fn_in_file = |fid: u32, name: &str| -> Option<u32> {
        let hits: Vec<u32> = name_index
            .get(name)?
            .iter()
            .copied()
            .filter(|&id| {
                functions[id as usize].file_id == fid && !functions[id as usize].is_toplevel
            })
            .collect();
        (hits.len() == 1).then(|| hits[0])
    };
    // The handler is by convention the LAST argument (`router.post('/x',
    // validate(v), controller.create)`), so only idents at the highest
    // argument index are candidates — resolving an earlier middleware ident
    // instead would be confidently wrong.
    let max_idx = call
        .arg_lits
        .iter()
        .filter(|l| l.kind == LitKind::Ident && l.key.is_none())
        .map(|l| l.index)
        .max()?;
    for lit in call
        .arg_lits
        .iter()
        .filter(|l| l.kind == LitKind::Ident && l.key.is_none() && l.index == max_idx)
    {
        let name = lit.text.rsplit(['.', ':']).next().unwrap_or(&lit.text);
        // Same-file first: `app.get("/x", listUsers)`.
        if let Some(id) = fn_in_file(func.file_id, name) {
            return Some(id);
        }
        // Import-following: `router.get('/', userController.list)` — the
        // dotted base (or the plain ident itself) bound by an import whose
        // module resolved to an indexed file; the method resolves there.
        let base = lit.text.split(['.', ':']).next().unwrap_or(&lit.text);
        if let Some(target) = file
            .imports
            .iter()
            .find(|i| i.names.iter().any(|n| n == base))
            .and_then(|i| i.resolved_file)
        {
            if target != func.file_id {
                if let Some(id) = fn_in_file(target, name) {
                    return Some(id);
                }
            }
        }
    }
    None
}

fn first_str_lit(call: &RawCall) -> Option<String> {
    call.arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none())
        .map(|l| l.text.clone())
}

fn nth_str_lit(call: &RawCall, n: usize) -> Option<String> {
    call.arg_lits
        .iter()
        .filter(|l| l.kind == LitKind::Str && l.key.is_none())
        .nth(n)
        .map(|l| l.text.clone())
}

fn str_lit_by_key(call: &RawCall, keys: &[&str]) -> Option<String> {
    call.arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.as_deref().is_some_and(|k| keys.contains(&k)))
        .map(|l| l.text.clone())
        // JS/TS object-literal fields are not keyed by the harvester: they
        // arrive as an Ident (the key) immediately followed by the value
        // literal at the same argument index. Accept that shape too — the key
        // name must match exactly, which keeps false pairings unlikely.
        .or_else(|| {
            call.arg_lits.windows(2).find_map(|w| {
                (w[0].kind == LitKind::Ident
                    && keys.contains(&w[0].text.as_str())
                    && w[1].kind == LitKind::Str
                    && w[1].key.is_none()
                    && w[1].index == w[0].index)
                    .then(|| w[1].text.clone())
            })
        })
}

fn first_path_lit(call: &RawCall) -> Option<String> {
    first_str_lit(call).filter(|s| s.starts_with('/'))
}

/// First unkeyed string literal that starts with '/', at any position.
fn path_shaped_lit(call: &RawCall) -> Option<String> {
    call.arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none() && l.text.starts_with('/'))
        .map(|l| l.text.clone())
}

fn first_url_lit(call: &RawCall) -> Option<String> {
    first_str_lit(call).filter(|s| url_shaped(s))
}

/// First unkeyed URL-shaped string literal, at any position (covers args
/// harvested one level down, e.g. Ruby `URI("https://...")`).
fn any_url_lit(call: &RawCall) -> Option<String> {
    call.arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none() && url_shaped(&l.text))
        .map(|l| l.text.clone())
}

fn first_deco_str(d: &RawDecoration) -> Option<String> {
    d.arg_lits
        .iter()
        .find(|l| l.kind == LitKind::Str && l.key.is_none())
        .map(|l| l.text.clone())
}

fn url_shaped(s: &str) -> bool {
    !s.contains(' ') && (s.starts_with('/') || s.contains("://"))
}

fn ensure_slash(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }
}

/// `("users", ":id")` -> `/users/:id`; tolerates slashes on either side.
fn join_prefix(prefix: &str, sub: &str) -> String {
    let p = prefix.trim_matches('/');
    let s = sub.trim_start_matches('/');
    match (p.is_empty(), s.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{s}"),
        (false, true) => format!("/{p}"),
        (false, false) => format!("/{p}/{s}"),
    }
}

/// Canonical form: lowercase segments, every parameter/interpolation segment
/// folded to `{*}`, scheme+host+query stripped. `/Users/{id}/` and
/// `/users/:id` both become `/users/{*}`.
pub fn normalize_path(raw: &str) -> Option<String> {
    let mut p = raw.trim();
    if p.is_empty() {
        return None;
    }
    if let Some(pos) = p.find("://") {
        let after = &p[pos + 3..];
        p = match after.find('/') {
            Some(i) => &after[i..],
            None => "/",
        };
    }
    if let Some(i) = p.find(['?', '#']) {
        // `#{` is Ruby interpolation, not a fragment marker.
        if !p[i..].starts_with("#{") {
            p = &p[..i];
        }
    }
    let segs: Vec<String> = p
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let param = s.starts_with(':')
                || s.starts_with('<')
                || s.starts_with('*')
                || s.starts_with('$')
                || s.contains('{')
                || s.contains("${");
            if param {
                "{*}".to_string()
            } else {
                s.to_ascii_lowercase()
            }
        })
        .collect();
    Some(format!("/{}", segs.join("/")))
}

fn concrete_segments(norm: &str) -> usize {
    norm.split('/')
        .filter(|s| !s.is_empty() && *s != "{*}")
        .count()
}

fn segs(norm: &str) -> Vec<&str> {
    norm.split('/').filter(|s| !s.is_empty()).collect()
}

fn unify(a: &[&str], b: &[&str]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x == y || *x == "{*}" || *y == "{*}")
}

/// Matching ladder: exact/unified path + method (unique -> High), then
/// suffix match with unknown client prefix (-> Heuristic). Method `Any` on
/// either side, a Heuristic endpoint, or a Heuristic client detection caps
/// at Heuristic. Suffix matches need >= 2 concrete segments on the endpoint
/// side to avoid `/{*}`-tail collisions.
/// Re-run correlation after endpoints have been appended post-detect() —
/// IaC-declared routes attach in the indexer, after the graph is built, and
/// would otherwise be invisible to matches/blast_radius.
pub fn recorrelate(idx: &mut EndpointIndex) {
    idx.matches = correlate(&idx.endpoints, &idx.client_calls);
}

fn correlate(endpoints: &[Endpoint], clients: &[ClientCall]) -> Vec<(u32, u32, Confidence)> {
    let mut out = Vec::new();
    for c in clients {
        // RPC-style calls correlate by protocol family + operation name
        // (gRPC: service name); path-shape tiers make no sense there.
        if c.kind != ApiKind::Http {
            let hits: Vec<&Endpoint> = endpoints
                .iter()
                .filter(|e| e.kind == c.kind && e.path_norm == c.path_norm)
                .collect();
            let unique = hits.len() == 1;
            for e in hits {
                let conf = if unique
                    && e.confidence == Confidence::High
                    && c.confidence == Confidence::High
                {
                    Confidence::High
                } else {
                    Confidence::Heuristic
                };
                out.push((c.id, e.id, conf));
            }
            continue;
        }
        let c_segs = segs(&c.path_norm);
        let tier1: Vec<&Endpoint> = endpoints
            .iter()
            .filter(|e| {
                e.kind == ApiKind::Http
                    && HttpMethod::compatible(e.method, c.method)
                    && unify(&segs(&e.path_norm), &c_segs)
            })
            .collect();
        if !tier1.is_empty() {
            let unique = tier1.len() == 1;
            for e in tier1 {
                let any = e.method == HttpMethod::Any || c.method == HttpMethod::Any;
                let conf = if unique
                    && !any
                    && e.confidence == Confidence::High
                    && c.confidence == Confidence::High
                {
                    Confidence::High
                } else {
                    Confidence::Heuristic
                };
                out.push((c.id, e.id, conf));
            }
            continue;
        }
        // Tier 2: strip leading client segments (unknown baseURL / proxy /
        // mount prefix on the caller side).
        for strip in 1..c_segs.len() {
            let tail = &c_segs[strip..];
            let hits: Vec<&Endpoint> = endpoints
                .iter()
                .filter(|e| {
                    e.kind == ApiKind::Http
                        && concrete_segments(&e.path_norm) >= 2
                        && HttpMethod::compatible(e.method, c.method)
                        && unify(&segs(&e.path_norm), tail)
                })
                .collect();
            if !hits.is_empty() {
                for e in hits {
                    out.push((c.id, e.id, Confidence::Heuristic));
                }
                break;
            }
        }
    }
    out
}
