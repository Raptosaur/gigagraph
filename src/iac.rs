//! Infrastructure-as-Code endpoint scanning: CloudFormation/SAM templates,
//! serverless.yml, and Terraform .tf files declare API routes and Lambda
//! handlers outside any tree-sitter language. `scan` lifts them into findings
//! per file (pure text -> findings, safe on malformed input); `attach`
//! appends them to `graph.endpoints` after the graph build, resolving handler
//! strings ("src/handlers/users.create", "app.lambda_handler") to indexed
//! functions, then re-runs correlation so IaC routes join in-repo client
//! calls.

use crate::endpoints::{self, ApiKind, Endpoint, HttpMethod};
use crate::graph::GigaGraph;
use crate::types::Confidence;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_norway::Value as Y;

#[derive(Debug, Clone)]
pub struct IacFinding {
    pub kind: ApiKind,
    pub method: HttpMethod,
    /// Raw route path ("/users/{id}"), GraphQL operation ("Query.getUser"),
    /// or "lambda:<Name>" for route-less Lambda entry points.
    pub path_raw: String,
    pub framework: &'static str,
    pub line: u32,
    pub confidence: Confidence,
    pub handler: Option<HandlerRef>,
    /// Route-less Lambda entry: pure noise unless the handler resolves to an
    /// indexed function, so `attach` drops it otherwise.
    pub require_handler: bool,
}

/// A handler string plus the context needed to resolve it after the graph
/// exists (resolution needs path_index/name_index).
#[derive(Debug, Clone)]
pub struct HandlerRef {
    /// Code root relative to the repo root (IaC file dir joined with
    /// CodeUri/source_dir), `/`-separated, normalized.
    pub base_dir: String,
    /// Handler as written ("src/handlers/users.create", "app.lambda_handler").
    pub handler: String,
    pub runtime: Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Node,
    Python,
    Ruby,
    /// Go/Java/.NET/container images: handler strings do not name a source
    /// file — resolution is skipped.
    Other,
    Unknown,
}

fn runtime_from(s: &str) -> Runtime {
    let s = s.to_ascii_lowercase();
    if s.starts_with("nodejs") {
        Runtime::Node
    } else if s.starts_with("python") {
        Runtime::Python
    } else if s.starts_with("ruby") {
        Runtime::Ruby
    } else {
        Runtime::Other
    }
}

// ---------------------------------------------------------------- dispatch

/// Pure text -> findings. Cheap for non-IaC files: dispatches on filename and
/// a substring pre-filter before any real parse; every parse failure yields
/// an empty vec (malformed YAML/HCL in repos is common).
pub fn scan(rel_path: &str, source: &str) -> Vec<IacFinding> {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let dir = parent_dir(rel_path);
    if name.ends_with(".tf") {
        return scan_terraform(dir, source);
    }
    if name == "serverless.yml" || name == "serverless.yaml" {
        return scan_serverless(dir, source);
    }
    let yamlish = name.ends_with(".yml") || name.ends_with(".yaml") || name.ends_with(".json");
    if yamlish && source.contains("Resources") && source.contains("AWS::") {
        return scan_cloudformation(dir, source);
    }
    Vec::new()
}

// -------------------------------------------------------------- YAML utils

fn untag(v: &Y) -> &Y {
    match v {
        Y::Tagged(t) => &t.value,
        _ => v,
    }
}

fn get<'a>(v: &'a Y, key: &str) -> Option<&'a Y> {
    match untag(v) {
        Y::Mapping(m) => m.get(key),
        _ => None,
    }
}

fn as_str(v: &Y) -> Option<&str> {
    match untag(v) {
        Y::String(s) => Some(s),
        _ => None,
    }
}

fn as_map(v: &Y) -> Option<&serde_norway::Mapping> {
    match untag(v) {
        Y::Mapping(m) => Some(m),
        _ => None,
    }
}

/// First line whose content starts with `<key>:`, 1-based; best-effort
/// (serde gives no spans).
fn line_of(source: &str, key: &str) -> u32 {
    let pat = format!("{key}:");
    for (i, l) in source.lines().enumerate() {
        if l.trim_start().starts_with(&pat) {
            return (i + 1) as u32;
        }
    }
    1
}

/// Logical-id target of an intrinsic in short (`!Ref X`, `!GetAtt X.Arn`,
/// `!Sub ...${X.Arn}...`, `!Join`) or long ({"Ref": "X"}, {"Fn::GetAtt": ..})
/// form.
fn logical_target(v: &Y) -> Option<String> {
    match v {
        Y::Tagged(t) => {
            if t.tag == "Ref" {
                as_str(&t.value).map(str::to_string)
            } else if t.tag == "GetAtt" {
                getatt_id(&t.value)
            } else if t.tag == "Sub" {
                sub_target(&t.value)
            } else if t.tag == "Join" {
                join_target(&t.value)
            } else {
                None
            }
        }
        Y::Mapping(m) => {
            if let Some(x) = m.get("Ref") {
                return as_str(x).map(str::to_string);
            }
            if let Some(x) = m.get("Fn::GetAtt") {
                return getatt_id(x);
            }
            if let Some(x) = m.get("Fn::Sub") {
                return sub_target(x);
            }
            if let Some(x) = m.get("Fn::Join") {
                return join_target(x);
            }
            None
        }
        Y::String(s) => extract_interp_ref(s),
        _ => None,
    }
}

fn getatt_id(v: &Y) -> Option<String> {
    match untag(v) {
        Y::String(s) => s.split('.').next().map(str::to_string),
        Y::Sequence(seq) => seq.first().and_then(as_str).map(str::to_string),
        _ => None,
    }
}

fn sub_target(v: &Y) -> Option<String> {
    // `!Sub "..."` or `!Sub ["...", {vars}]`.
    let text = match untag(v) {
        Y::String(s) => s.as_str(),
        Y::Sequence(seq) => seq.first().and_then(as_str)?,
        _ => return None,
    };
    extract_interp_ref(text)
}

fn join_target(v: &Y) -> Option<String> {
    // `!Join [sep, [parts]]`: scan parts for a nested intrinsic target.
    let Y::Sequence(seq) = untag(v) else {
        return None;
    };
    let Y::Sequence(parts) = untag(seq.get(1)?) else {
        return None;
    };
    parts.iter().find_map(logical_target)
}

/// First `${LogicalId.Arn}` — or plain `${LogicalId}` — in a Sub template.
/// Pseudo-parameters (`${AWS::Region}`) are ident-gated out; callers verify
/// the id against the resource catalog anyway.
fn extract_interp_ref(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find("${") {
        let after = &rest[i + 2..];
        let end = after.find('}')?;
        let inner = &after[..end];
        let id = inner.strip_suffix(".Arn").unwrap_or(inner);
        if !id.is_empty()
            && !id.contains(['.', ':', '!'])
            && id.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            return Some(id.to_string());
        }
        rest = &after[end + 1..];
    }
    None
}

/// `"GET /users/{id}"` | `"ANY /x"` | `"$default"`.
fn parse_route_key(rk: &str) -> Option<(HttpMethod, String)> {
    let rk = rk.trim();
    if rk == "$default" {
        return Some((HttpMethod::Any, "$default".to_string()));
    }
    let (m, p) = rk.split_once(' ')?;
    Some((HttpMethod::from_name(m)?, p.trim().to_string()))
}

fn path_conf(path: &str) -> Confidence {
    // Unexpanded template variables stay raw; normalize folds them to {*}.
    if path.contains("${") {
        Confidence::Heuristic
    } else {
        Confidence::High
    }
}

// -------------------------------------------------- CloudFormation and SAM

/// REST resource parent link: `!GetAtt Api.RootResourceId` terminates at the
/// API root; `!Ref OtherResource` chains; imports/parameters are Unknown and
/// surface as a `{*}` path prefix.
#[derive(Debug, Clone, PartialEq)]
enum Parent {
    Root,
    Res(String),
    Unknown,
}

fn parse_parent(v: &Y) -> Parent {
    let getatt_root = |x: &Y| -> Parent {
        let attr = match untag(x) {
            Y::String(s) => s.rsplit('.').next().map(str::to_string),
            Y::Sequence(seq) => seq.last().and_then(as_str).map(str::to_string),
            _ => None,
        };
        if attr.as_deref() == Some("RootResourceId") {
            Parent::Root
        } else {
            Parent::Unknown
        }
    };
    match v {
        Y::Tagged(t) if t.tag == "Ref" => as_str(&t.value)
            .map(|s| Parent::Res(s.to_string()))
            .unwrap_or(Parent::Unknown),
        Y::Tagged(t) if t.tag == "GetAtt" => getatt_root(&t.value),
        Y::Mapping(m) => {
            if let Some(x) = m.get("Ref") {
                return as_str(x)
                    .map(|s| Parent::Res(s.to_string()))
                    .unwrap_or(Parent::Unknown);
            }
            if let Some(x) = m.get("Fn::GetAtt") {
                return getatt_root(x);
            }
            Parent::Unknown
        }
        _ => Parent::Unknown,
    }
}

/// Walk parent links to the root, innermost segment last. An unresolvable
/// link (ImportValue, Parameter, missing resource) leaves a `{*}` prefix and
/// caps at Heuristic.
fn assemble_rest_path(
    parts: &FxHashMap<&str, (String, Parent)>,
    start: &Parent,
) -> (String, Confidence) {
    let mut segs: Vec<String> = Vec::new();
    let mut cur = start.clone();
    for _ in 0..32 {
        match cur {
            Parent::Root => {
                segs.reverse();
                return (format!("/{}", segs.join("/")), Confidence::High);
            }
            Parent::Res(ref id) => match parts.get(id.as_str()) {
                Some((part, parent)) => {
                    segs.push(part.clone());
                    cur = parent.clone();
                }
                None => break,
            },
            Parent::Unknown => break,
        }
    }
    segs.push("{*}".to_string());
    segs.reverse();
    (format!("/{}", segs.join("/")), Confidence::Heuristic)
}

struct CfnRes<'a> {
    rtype: &'a str,
    props: Option<&'a Y>,
    meta: Option<&'a Y>,
}

struct SamGlobals<'a> {
    handler: Option<&'a str>,
    codeuri: Option<&'a str>,
    runtime: Option<&'a str>,
}

/// Handler reference for a Lambda-shaped resource, when its code is locally
/// resolvable: SAM functions via Handler+CodeUri (Globals merged, resource
/// wins), raw Lambda functions only via CDK-synth `aws:asset.path` metadata.
fn cfn_lambda_ref(dir: &str, res: &CfnRes, g: &SamGlobals) -> Option<HandlerRef> {
    match res.rtype {
        "AWS::Serverless::Function" => {
            let handler = res
                .props
                .and_then(|p| get(p, "Handler"))
                .and_then(as_str)
                .or(g.handler)?;
            let codeuri = match res.props.and_then(|p| get(p, "CodeUri")) {
                // Present but not a string (S3 {Bucket,Key} form) -> no
                // local code; do NOT fall back to Globals.
                Some(v) => as_str(v)?,
                None => g.codeuri.unwrap_or("."),
            };
            if codeuri.contains("://") {
                return None;
            }
            let runtime = res
                .props
                .and_then(|p| get(p, "Runtime"))
                .and_then(as_str)
                .or(g.runtime)
                .map(runtime_from)
                .unwrap_or(Runtime::Unknown);
            let mut handler = handler.to_string();
            // esbuild-built functions: the real module is the entry point;
            // the Handler stem names the bundle output.
            if let Some(m) = res.meta {
                let esbuild = get(m, "BuildMethod").and_then(as_str) == Some("esbuild");
                if esbuild {
                    let entry = get(m, "BuildProperties")
                        .and_then(|bp| get(bp, "EntryPoints"))
                        .and_then(|e| match untag(e) {
                            Y::Sequence(sq) => sq.first(),
                            _ => None,
                        })
                        .and_then(as_str);
                    if let Some(ep) = entry {
                        let export = handler.rsplit('.').next().unwrap_or("handler").to_string();
                        handler = format!("{}.{export}", strip_node_ext(ep));
                    }
                }
            }
            Some(HandlerRef {
                base_dir: join_norm(dir, codeuri),
                handler,
                runtime,
            })
        }
        "AWS::Lambda::Function" => {
            let props = res.props?;
            let handler = get(props, "Handler").and_then(as_str)?;
            let asset = res.meta.and_then(|m| get(m, "aws:asset.path")).and_then(as_str)?;
            let runtime = get(props, "Runtime")
                .and_then(as_str)
                .map(runtime_from)
                .unwrap_or(Runtime::Unknown);
            Some(HandlerRef {
                base_dir: join_norm(dir, asset),
                handler: handler.to_string(),
                runtime,
            })
        }
        _ => None,
    }
}

fn scan_cloudformation(dir: &str, source: &str) -> Vec<IacFinding> {
    let Ok(doc) = serde_norway::from_str::<Y>(source) else {
        return Vec::new();
    };
    let Some(res_map) = get(&doc, "Resources").and_then(as_map) else {
        return Vec::new();
    };

    let globals_fn = get(&doc, "Globals").and_then(|g| get(g, "Function"));
    let globals = SamGlobals {
        handler: globals_fn.and_then(|g| get(g, "Handler")).and_then(as_str),
        codeuri: globals_fn.and_then(|g| get(g, "CodeUri")).and_then(as_str),
        runtime: globals_fn.and_then(|g| get(g, "Runtime")).and_then(as_str),
    };

    let mut catalog: FxHashMap<&str, CfnRes> = FxHashMap::default();
    for (k, v) in res_map {
        let Y::String(name) = k else { continue };
        let Some(rtype) = get(v, "Type").and_then(as_str) else {
            continue;
        };
        catalog.insert(
            name.as_str(),
            CfnRes {
                rtype,
                props: get(v, "Properties"),
                meta: get(v, "Metadata"),
            },
        );
    }
    let lambda_ref = |id: &str| -> Option<HandlerRef> {
        catalog
            .get(id)
            .and_then(|r| cfn_lambda_ref(dir, r, &globals))
    };

    let mut out = Vec::new();
    // Lambda-shaped resources bound to some route; the rest become
    // `lambda:` entry-point findings at the end.
    let mut routed: FxHashSet<String> = FxHashSet::default();

    // SAM function HTTP events.
    for (name, res) in &catalog {
        if res.rtype != "AWS::Serverless::Function" {
            continue;
        }
        let href = cfn_lambda_ref(dir, res, &globals);
        let line = line_of(source, name);
        let mut had_http = false;
        if let Some(events) = res.props.and_then(|p| get(p, "Events")).and_then(as_map) {
            for (_, ev) in events {
                let Some(ev_type) = get(ev, "Type").and_then(as_str) else {
                    continue;
                };
                if ev_type != "Api" && ev_type != "HttpApi" {
                    continue;
                }
                let ep = get(ev, "Properties");
                let path = ep.and_then(|p| get(p, "Path")).and_then(as_str);
                let method = ep.and_then(|p| get(p, "Method")).and_then(as_str);
                let (m, p) = match (method, path) {
                    (Some(m), Some(p)) => (
                        HttpMethod::from_name(m).unwrap_or(HttpMethod::Any),
                        p.to_string(),
                    ),
                    // HttpApi event with no Path/Method = $default catch-all.
                    _ if ev_type == "HttpApi" => (HttpMethod::Any, "$default".to_string()),
                    _ => continue,
                };
                out.push(IacFinding {
                    kind: ApiKind::Http,
                    method: m,
                    confidence: path_conf(&p),
                    path_raw: p,
                    framework: "sam",
                    line,
                    handler: href.clone(),
                    require_handler: false,
                });
                had_http = true;
            }
        }
        if had_http {
            routed.insert(name.to_string());
        }
    }

    // ApiGatewayV2 routes: Target -> Integration -> IntegrationUri -> lambda.
    for (name, res) in &catalog {
        if res.rtype != "AWS::ApiGatewayV2::Route" {
            continue;
        }
        let Some(props) = res.props else { continue };
        let Some((method, path)) = get(props, "RouteKey")
            .and_then(as_str)
            .and_then(parse_route_key)
        else {
            continue;
        };
        let lambda = get(props, "Target")
            .and_then(logical_target)
            .and_then(|iid| catalog.get(iid.as_str()))
            .filter(|i| i.rtype == "AWS::ApiGatewayV2::Integration")
            .and_then(|i| i.props)
            .and_then(|ip| get(ip, "IntegrationUri"))
            .and_then(logical_target);
        let href = lambda.as_deref().and_then(lambda_ref);
        if let Some(l) = lambda {
            routed.insert(l);
        }
        out.push(IacFinding {
            kind: ApiKind::Http,
            method,
            confidence: path_conf(&path),
            path_raw: path,
            framework: "cloudformation",
            line: line_of(source, name),
            handler: href,
            require_handler: false,
        });
    }

    // REST API: Resource parent chains + Methods.
    let mut rest_parts: FxHashMap<&str, (String, Parent)> = FxHashMap::default();
    for (name, res) in &catalog {
        if res.rtype != "AWS::ApiGateway::Resource" {
            continue;
        }
        let Some(props) = res.props else { continue };
        let Some(part) = get(props, "PathPart").and_then(as_str) else {
            continue;
        };
        let parent = get(props, "ParentId")
            .map(parse_parent)
            .unwrap_or(Parent::Unknown);
        rest_parts.insert(name, (part.to_string(), parent));
    }
    for (name, res) in &catalog {
        if res.rtype != "AWS::ApiGateway::Method" {
            continue;
        }
        let Some(props) = res.props else { continue };
        let Some(method) = get(props, "HttpMethod")
            .and_then(as_str)
            .and_then(HttpMethod::from_name)
        else {
            continue;
        };
        let resource = get(props, "ResourceId")
            .map(parse_parent)
            .unwrap_or(Parent::Unknown);
        let (path, conf) = assemble_rest_path(&rest_parts, &resource);
        let lambda = get(props, "Integration")
            .and_then(|i| get(i, "Uri"))
            .and_then(logical_target);
        let href = lambda.as_deref().and_then(lambda_ref);
        if let Some(l) = lambda {
            routed.insert(l);
        }
        out.push(IacFinding {
            kind: ApiKind::Http,
            method,
            path_raw: path,
            framework: "cloudformation",
            line: line_of(source, name),
            confidence: conf,
            handler: href,
            require_handler: false,
        });
    }

    // AppSync resolvers -> Graphql operations, lambda-backed when the data
    // source is AWS_LAMBDA with a same-template function ARN.
    for (name, res) in &catalog {
        if res.rtype != "AWS::AppSync::Resolver" {
            continue;
        }
        let Some(props) = res.props else { continue };
        let (Some(t), Some(f)) = (
            get(props, "TypeName").and_then(as_str),
            get(props, "FieldName").and_then(as_str),
        ) else {
            continue;
        };
        let ds = get(props, "DataSourceName");
        // `!GetAtt DS.Name` names the resource; a plain string names the
        // data source's Properties.Name.
        let ds_res = ds
            .and_then(logical_target)
            .and_then(|id| catalog.get(id.as_str()).map(|r| (id.clone(), r)))
            .or_else(|| {
                let wanted = ds.and_then(as_str)?;
                catalog.iter().find_map(|(id, r)| {
                    (r.rtype == "AWS::AppSync::DataSource"
                        && r.props.and_then(|p| get(p, "Name")).and_then(as_str) == Some(wanted))
                    .then(|| (id.to_string(), r))
                })
            });
        let lambda = ds_res
            .filter(|(_, r)| {
                r.rtype == "AWS::AppSync::DataSource"
                    && r.props.and_then(|p| get(p, "Type")).and_then(as_str) == Some("AWS_LAMBDA")
            })
            .and_then(|(_, r)| r.props)
            .and_then(|p| get(p, "LambdaConfig"))
            .and_then(|lc| get(lc, "LambdaFunctionArn"))
            .and_then(logical_target);
        let href = lambda.as_deref().and_then(lambda_ref);
        if let Some(l) = lambda {
            routed.insert(l);
        }
        out.push(IacFinding {
            kind: ApiKind::Graphql,
            method: HttpMethod::Any,
            path_raw: format!("{t}.{f}"),
            framework: "appsync",
            line: line_of(source, name),
            confidence: Confidence::High,
            handler: href,
            require_handler: false,
        });
    }

    // Route-less lambdas (SQS/schedule/bare): visible only when the handler
    // resolves in-repo (require_handler gates in attach).
    for (name, res) in &catalog {
        if routed.contains(*name)
            || !matches!(
                res.rtype,
                "AWS::Serverless::Function" | "AWS::Lambda::Function"
            )
        {
            continue;
        }
        let Some(href) = cfn_lambda_ref(dir, res, &globals) else {
            continue;
        };
        out.push(IacFinding {
            kind: ApiKind::Http,
            method: HttpMethod::Any,
            path_raw: format!("lambda:{name}"),
            framework: "lambda",
            line: line_of(source, name),
            confidence: Confidence::Heuristic,
            handler: Some(href),
            require_handler: true,
        });
    }
    out
}

// ------------------------------------------------------------ serverless.yml

/// `http`/`httpApi` event: object `{path, method}` or shorthand string
/// `"GET /users"`; `'*'` is the $default catch-all.
fn sls_http_event(v: &Y) -> Option<(HttpMethod, String)> {
    match untag(v) {
        Y::String(s) => {
            let s = s.trim();
            if s == "*" {
                return Some((HttpMethod::Any, "$default".to_string()));
            }
            match s.split_once(' ') {
                Some((m, p)) => Some((HttpMethod::from_name(m)?, p.trim().to_string())),
                None => Some((HttpMethod::Any, s.to_string())),
            }
        }
        Y::Mapping(_) => {
            let path = get(v, "path").and_then(as_str)?;
            let method = get(v, "method")
                .and_then(as_str)
                .and_then(HttpMethod::from_name)
                .unwrap_or(HttpMethod::Any);
            Some((method, path.to_string()))
        }
        _ => None,
    }
}

fn scan_serverless(dir: &str, source: &str) -> Vec<IacFinding> {
    let Ok(doc) = serde_norway::from_str::<Y>(source) else {
        return Vec::new();
    };
    // Corroborate: a serverless.yml has top-level `service` + `provider`.
    if get(&doc, "service").is_none() || get(&doc, "provider").is_none() {
        return Vec::new();
    }
    let provider_runtime = get(&doc, "provider")
        .and_then(|p| get(p, "runtime"))
        .and_then(as_str);

    let mut out = Vec::new();
    if let Some(functions) = get(&doc, "functions").and_then(as_map) {
        for (k, fdef) in functions {
            let Y::String(fname) = k else { continue };
            let line = line_of(source, fname);
            let runtime = get(fdef, "runtime")
                .and_then(as_str)
                .or(provider_runtime)
                .map(runtime_from)
                .unwrap_or(Runtime::Unknown);
            // Handler paths are relative to the serverless.yml's directory.
            let href = get(fdef, "handler").and_then(as_str).map(|h| HandlerRef {
                base_dir: dir.to_string(),
                handler: h.to_string(),
                runtime,
            });
            let mut had_http = false;
            if let Some(Y::Sequence(events)) = get(fdef, "events").map(untag) {
                for ev in events {
                    let hit = get(ev, "http")
                        .or_else(|| get(ev, "httpApi"))
                        .and_then(sls_http_event);
                    let Some((method, path)) = hit else { continue };
                    out.push(IacFinding {
                        kind: ApiKind::Http,
                        method,
                        confidence: path_conf(&path),
                        path_raw: path,
                        framework: "serverless",
                        line,
                        handler: href.clone(),
                        require_handler: false,
                    });
                    had_http = true;
                }
            }
            // Lambda function URL: a real catch-all endpoint.
            let url = get(fdef, "url")
                .map(|u| !matches!(untag(u), Y::Bool(false)))
                .unwrap_or(false);
            if url {
                out.push(IacFinding {
                    kind: ApiKind::Http,
                    method: HttpMethod::Any,
                    path_raw: "/{*}".to_string(),
                    framework: "serverless",
                    line,
                    confidence: Confidence::High,
                    handler: href.clone(),
                    require_handler: false,
                });
                had_http = true;
            }
            if !had_http && href.is_some() {
                out.push(IacFinding {
                    kind: ApiKind::Http,
                    method: HttpMethod::Any,
                    path_raw: format!("lambda:{fname}"),
                    framework: "lambda",
                    line,
                    confidence: Confidence::Heuristic,
                    handler: href,
                    require_handler: true,
                });
            }
        }
    }

    // serverless-appsync plugin: v2 `appSync.resolvers` keyed "Type.field",
    // v1 `custom.appSync.mappingTemplates` [{type, field}].
    let mut push_op = |op: String, line: u32| {
        out.push(IacFinding {
            kind: ApiKind::Graphql,
            method: HttpMethod::Any,
            path_raw: op,
            framework: "serverless-appsync",
            line,
            confidence: Confidence::High,
            handler: None,
            require_handler: false,
        });
    };
    if let Some(rs) = get(&doc, "appSync").and_then(|a| get(a, "resolvers")).and_then(as_map) {
        for (k, _) in rs {
            let Y::String(op) = k else { continue };
            if op.contains('.') {
                push_op(op.clone(), line_of(source, op));
            }
        }
    }
    if let Some(Y::Sequence(mts)) = get(&doc, "custom")
        .and_then(|c| get(c, "appSync"))
        .and_then(|a| get(a, "mappingTemplates"))
        .map(untag)
    {
        for mt in mts {
            if let (Some(t), Some(f)) = (
                get(mt, "type").and_then(as_str),
                get(mt, "field").and_then(as_str),
            ) {
                push_op(format!("{t}.{f}"), line_of(source, f));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- Terraform

fn expr_string(e: &hcl::Expression) -> Option<String> {
    match e {
        hcl::Expression::String(s) => Some(s.clone()),
        // Interpolated strings stay raw ("${path.module}/src").
        hcl::Expression::TemplateExpr(t) => match &**t {
            hcl::TemplateExpr::QuotedString(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// `aws_lambda_function.fn.invoke_arn` -> "aws_lambda_function.fn.invoke_arn".
/// Index/splat operators (count/for_each) poison static resolution -> None.
fn expr_traversal(e: &hcl::Expression) -> Option<String> {
    let hcl::Expression::Traversal(t) = e else {
        return None;
    };
    let hcl::Expression::Variable(v) = &t.expr else {
        return None;
    };
    let mut parts = vec![v.as_str().to_string()];
    for op in &t.operators {
        match op {
            hcl::TraversalOperator::GetAttr(id) => parts.push(id.as_str().to_string()),
            _ => return None,
        }
    }
    Some(parts.join("."))
}

/// `"…${<prefix>.<name>.…}…"` -> name.
fn tpl_ref(text: &str, prefix: &str) -> Option<String> {
    let i = text.find(prefix)?;
    let rest = text[i + prefix.len()..].strip_prefix('.')?;
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

/// Resource-label target of a traversal or interpolation, by resource type.
fn ref_of(e: &hcl::Expression, res_type: &str) -> Option<String> {
    if let Some(t) = expr_traversal(e) {
        let mut it = t.split('.');
        return (it.next() == Some(res_type)).then(|| it.next().map(str::to_string))?;
    }
    expr_string(e).and_then(|s| tpl_ref(&s, res_type))
}

fn data_ref(e: &hcl::Expression, dtype: &str) -> Option<String> {
    if let Some(t) = expr_traversal(e) {
        let segs: Vec<&str> = t.split('.').collect();
        return (segs.len() >= 3 && segs[0] == "data" && segs[1] == dtype)
            .then(|| segs[2].to_string());
    }
    expr_string(e).and_then(|s| tpl_ref(&s, &format!("data.{dtype}")))
}

fn attr<'a>(body: &'a hcl::Body, key: &str) -> Option<&'a hcl::Expression> {
    body.attributes().find(|a| a.key() == key).map(|a| a.expr())
}

fn tf_parent(e: &hcl::Expression) -> Parent {
    if let Some(t) = expr_traversal(e) {
        let segs: Vec<&str> = t.split('.').collect();
        if segs.len() >= 3 && segs[0] == "aws_api_gateway_rest_api" && segs[2] == "root_resource_id"
        {
            return Parent::Root;
        }
        if segs.len() >= 2 && segs[0] == "aws_api_gateway_resource" {
            return Parent::Res(segs[1].to_string());
        }
    }
    Parent::Unknown
}

/// `"${path.module}/src"` -> repo-relative dir under the .tf file's dir;
/// other unexpanded interpolation -> None.
fn module_path(dir: &str, raw: &str) -> Option<String> {
    let raw = raw.trim();
    let rel = raw
        .strip_prefix("${path.module}")
        .map(|r| r.trim_start_matches('/'))
        .unwrap_or(raw);
    if rel.contains("${") {
        return None;
    }
    Some(join_norm(dir, rel))
}

/// First line containing the needle, 1-based; best-effort (hcl-rs keeps no
/// spans in the plain Body API).
fn line_containing(source: &str, needle: &str) -> u32 {
    for (i, l) in source.lines().enumerate() {
        if l.contains(needle) {
            return (i + 1) as u32;
        }
    }
    1
}

fn scan_terraform(dir: &str, source: &str) -> Vec<IacFinding> {
    // Cheap pre-filter before a full HCL parse.
    if !source.contains("resource") {
        return Vec::new();
    }
    let Ok(body) = hcl::from_str::<hcl::Body>(source) else {
        return Vec::new();
    };

    struct TfLambda {
        handler: Option<String>,
        runtime: Option<String>,
        archive: Option<String>,
        line: u32,
    }
    let mut lambdas: FxHashMap<String, TfLambda> = FxHashMap::default();
    let mut archives: FxHashMap<String, String> = FxHashMap::default();
    let mut integrations: FxHashMap<String, String> = FxHashMap::default();
    let mut routes: Vec<(String, Option<String>, u32)> = Vec::new();
    let mut quick: Vec<(String, Option<String>, u32)> = Vec::new();
    let mut rest_parts_owned: Vec<(String, String, Parent)> = Vec::new();
    let mut rest_methods: Vec<(Parent, String, u32)> = Vec::new();
    let mut rest_ints: Vec<(Parent, Option<String>, Option<String>)> = Vec::new();
    let mut resolvers: Vec<(String, Option<String>, u32)> = Vec::new();
    let mut datasources: FxHashMap<String, Option<String>> = FxHashMap::default();
    let mut func_urls: Vec<(Option<String>, u32)> = Vec::new();

    for block in body.blocks() {
        let labels = block.labels();
        let b = block.body();
        match block.identifier() {
            "resource" if labels.len() >= 2 => {
                let rtype = labels[0].as_str();
                let name = labels[1].as_str().to_string();
                let line = line_containing(source, &format!("\"{name}\""));
                match rtype {
                    "aws_apigatewayv2_route" => {
                        let Some(rk) = attr(b, "route_key").and_then(expr_string) else {
                            continue;
                        };
                        let target = attr(b, "target")
                            .and_then(|e| ref_of(e, "aws_apigatewayv2_integration"));
                        routes.push((rk, target, line));
                    }
                    "aws_apigatewayv2_integration" => {
                        if let Some(l) = attr(b, "integration_uri")
                            .and_then(|e| ref_of(e, "aws_lambda_function"))
                        {
                            integrations.insert(name, l);
                        }
                    }
                    // Quick-create API: inline route_key + lambda target.
                    "aws_apigatewayv2_api" => {
                        if let Some(rk) = attr(b, "route_key").and_then(expr_string) {
                            let lam =
                                attr(b, "target").and_then(|e| ref_of(e, "aws_lambda_function"));
                            quick.push((rk, lam, line));
                        }
                    }
                    "aws_api_gateway_resource" => {
                        let Some(part) = attr(b, "path_part").and_then(expr_string) else {
                            continue;
                        };
                        let parent = attr(b, "parent_id").map(tf_parent).unwrap_or(Parent::Unknown);
                        rest_parts_owned.push((name, part, parent));
                    }
                    "aws_api_gateway_method" => {
                        let Some(m) = attr(b, "http_method").and_then(expr_string) else {
                            continue;
                        };
                        let resource =
                            attr(b, "resource_id").map(tf_parent).unwrap_or(Parent::Unknown);
                        rest_methods.push((resource, m, line));
                    }
                    "aws_api_gateway_integration" => {
                        let resource =
                            attr(b, "resource_id").map(tf_parent).unwrap_or(Parent::Unknown);
                        // Often a traversal to the method's http_method ->
                        // None -> wildcard match on the resource.
                        let m = attr(b, "http_method").and_then(expr_string);
                        let lam = attr(b, "uri").and_then(|e| ref_of(e, "aws_lambda_function"));
                        rest_ints.push((resource, m, lam));
                    }
                    "aws_lambda_function" => {
                        lambdas.insert(
                            name,
                            TfLambda {
                                handler: attr(b, "handler").and_then(expr_string),
                                runtime: attr(b, "runtime").and_then(expr_string),
                                archive: attr(b, "filename")
                                    .and_then(|e| data_ref(e, "archive_file")),
                                line,
                            },
                        );
                    }
                    "aws_appsync_resolver" => {
                        let t = attr(b, "type").and_then(expr_string);
                        let f = attr(b, "field").and_then(expr_string);
                        let ds = attr(b, "data_source").and_then(|e| {
                            ref_of(e, "aws_appsync_datasource").or_else(|| expr_string(e))
                        });
                        if let (Some(t), Some(f)) = (t, f) {
                            resolvers.push((format!("{t}.{f}"), ds, line));
                        }
                    }
                    "aws_appsync_datasource" => {
                        let lam = b
                            .blocks()
                            .find(|bb| bb.identifier() == "lambda_config")
                            .and_then(|bb| attr(bb.body(), "function_arn"))
                            .and_then(|e| ref_of(e, "aws_lambda_function"));
                        // Keyed by both the resource label and the AppSync
                        // `name` attribute (resolvers may use either).
                        if let Some(n) = attr(b, "name").and_then(expr_string) {
                            datasources.insert(n, lam.clone());
                        }
                        datasources.insert(name, lam);
                    }
                    "aws_lambda_function_url" => {
                        let lam =
                            attr(b, "function_name").and_then(|e| ref_of(e, "aws_lambda_function"));
                        func_urls.push((lam, line));
                    }
                    _ => {}
                }
            }
            "data" if labels.len() >= 2 && labels[0].as_str() == "archive_file" => {
                let base = attr(b, "source_dir")
                    .and_then(expr_string)
                    .and_then(|r| module_path(dir, &r))
                    .or_else(|| {
                        attr(b, "source_file")
                            .and_then(expr_string)
                            .and_then(|r| module_path(dir, &r))
                            .map(|p| parent_dir(&p).to_string())
                    });
                if let Some(base) = base {
                    archives.insert(labels[1].as_str().to_string(), base);
                }
            }
            _ => {}
        }
    }

    let href_for = |lam: &Option<String>| -> Option<HandlerRef> {
        let l = lambdas.get(lam.as_deref()?)?;
        let handler = l.handler.clone()?;
        if handler.contains("${") {
            return None;
        }
        let runtime = l.runtime.as_deref().map(runtime_from).unwrap_or(Runtime::Unknown);
        // Without an archive_file chain, fall back to the .tf file's dir;
        // resolution still demands the file actually exists in the index.
        let base_dir = l
            .archive
            .as_ref()
            .and_then(|a| archives.get(a))
            .cloned()
            .unwrap_or_else(|| dir.to_string());
        Some(HandlerRef {
            base_dir,
            handler,
            runtime,
        })
    };

    let mut out = Vec::new();
    let mut routed: FxHashSet<String> = FxHashSet::default();
    let push_http =
        |out: &mut Vec<IacFinding>,
         routed: &mut FxHashSet<String>,
         method: HttpMethod,
         path: String,
         conf: Confidence,
         line: u32,
         lam: Option<String>| {
            let href = href_for(&lam);
            if let Some(l) = lam {
                routed.insert(l);
            }
            out.push(IacFinding {
                kind: ApiKind::Http,
                method,
                confidence: conf.min_with(path_conf(&path)),
                path_raw: path,
                framework: "terraform",
                line,
                handler: href,
                require_handler: false,
            });
        };

    for (rk, target, line) in routes {
        let Some((method, path)) = parse_route_key(&rk) else {
            continue;
        };
        let lam = target.and_then(|t| integrations.get(&t).cloned());
        push_http(&mut out, &mut routed, method, path, Confidence::High, line, lam);
    }
    for (rk, lam, line) in quick {
        let Some((method, path)) = parse_route_key(&rk) else {
            continue;
        };
        push_http(&mut out, &mut routed, method, path, Confidence::High, line, lam);
    }
    let rest_parts: FxHashMap<&str, (String, Parent)> = rest_parts_owned
        .iter()
        .map(|(n, p, par)| (n.as_str(), (p.clone(), par.clone())))
        .collect();
    for (resource, m, line) in rest_methods {
        let Some(method) = HttpMethod::from_name(&m) else {
            continue;
        };
        let (path, conf) = assemble_rest_path(&rest_parts, &resource);
        let lam = rest_ints
            .iter()
            .find(|(r, im, _)| *r == resource && im.as_deref().is_none_or(|im| im == m))
            .and_then(|(_, _, l)| l.clone());
        push_http(&mut out, &mut routed, method, path, conf, line, lam);
    }
    for (lam, line) in func_urls {
        push_http(
            &mut out,
            &mut routed,
            HttpMethod::Any,
            "/{*}".to_string(),
            Confidence::High,
            line,
            lam,
        );
    }
    for (op, ds, line) in resolvers {
        let lam = ds.and_then(|d| datasources.get(&d).cloned()).flatten();
        let href = href_for(&Some(lam.clone().unwrap_or_default()));
        if let Some(l) = lam {
            routed.insert(l);
        }
        out.push(IacFinding {
            kind: ApiKind::Graphql,
            method: HttpMethod::Any,
            path_raw: op,
            framework: "terraform",
            line,
            confidence: Confidence::High,
            handler: href,
            require_handler: false,
        });
    }
    for (name, l) in &lambdas {
        if routed.contains(name) {
            continue;
        }
        let Some(href) = href_for(&Some(name.clone())) else {
            continue;
        };
        out.push(IacFinding {
            kind: ApiKind::Http,
            method: HttpMethod::Any,
            path_raw: format!("lambda:{name}"),
            framework: "lambda",
            line: l.line,
            confidence: Confidence::Heuristic,
            handler: Some(href),
            require_handler: true,
        });
    }
    out
}

trait ConfMin {
    fn min_with(self, other: Confidence) -> Confidence;
}
impl ConfMin for Confidence {
    fn min_with(self, other: Confidence) -> Confidence {
        if self == Confidence::Heuristic || other == Confidence::Heuristic {
            Confidence::Heuristic
        } else {
            Confidence::High
        }
    }
}

// ------------------------------------------------------------------ attach

/// Appends IaC findings as endpoints (ids stay sequential — only ever push)
/// and re-correlates so IaC routes join in-repo client calls.
pub fn attach(graph: &mut GigaGraph, files: &[(String, Vec<IacFinding>)]) {
    let mut appended = false;
    for (rel, findings) in files {
        let Some(&file_id) = graph.path_index.get(rel.as_str()) else {
            continue;
        };
        for f in findings {
            let resolved = f.handler.as_ref().and_then(|h| resolve_handler(graph, h));
            if f.require_handler && resolved.is_none() {
                continue;
            }
            let (path_raw, path_norm) = match f.kind {
                // GraphQL operations mirror the RPC-op convention: raw op,
                // normalized "/lowercased-op".
                ApiKind::Graphql => {
                    let Some(n) = endpoints::normalize_path(&format!("/{}", f.path_raw)) else {
                        continue;
                    };
                    (f.path_raw.clone(), n)
                }
                _ if f.path_raw.starts_with("lambda:") => {
                    // Distinct first segment: never unifies with real client
                    // paths, so correlation pollution is negligible while
                    // blast_radius still gains the handler edge.
                    let name = f.path_raw["lambda:".len()..].to_ascii_lowercase();
                    let Some(n) = endpoints::normalize_path(&format!("/lambda/{name}")) else {
                        continue;
                    };
                    (f.path_raw.clone(), n)
                }
                _ if f.path_raw == "$default" => {
                    ("$default".to_string(), "/{*}".to_string())
                }
                _ => {
                    let raw = if f.path_raw.starts_with('/') {
                        f.path_raw.clone()
                    } else {
                        format!("/{}", f.path_raw)
                    };
                    let Some(n) = endpoints::normalize_path(&raw) else {
                        continue;
                    };
                    (raw, n)
                }
            };
            // A declared-but-unresolved handler string caps confidence.
            let confidence = if f.handler.is_some() && resolved.is_none() {
                Confidence::Heuristic
            } else {
                f.confidence
            };
            graph.endpoints.endpoints.push(Endpoint {
                id: graph.endpoints.endpoints.len() as u32,
                kind: f.kind,
                method: f.method,
                path_raw,
                path_norm,
                framework: f.framework.to_string(),
                file_id,
                line: f.line,
                handler: resolved,
                confidence,
            });
            appended = true;
        }
    }
    if appended {
        endpoints::recorrelate(&mut graph.endpoints);
    }
}

// ------------------------------------------------------ handler resolution

const NODE_EXTS: &[&str] = &[".js", ".mjs", ".cjs", ".jsx", ".ts", ".mts", ".cts", ".tsx"];

fn strip_node_ext(p: &str) -> &str {
    for ext in NODE_EXTS {
        if let Some(stem) = p.strip_suffix(ext) {
            return stem;
        }
    }
    p
}

fn resolve_handler(graph: &GigaGraph, h: &HandlerRef) -> Option<u32> {
    if h.handler.contains("${") || h.handler.contains("::") {
        return None;
    }
    let (module, export) = h.handler.rsplit_once('.')?;
    if module.is_empty() || export.is_empty() {
        return None;
    }
    match h.runtime {
        Runtime::Node => resolve_node(graph, &h.base_dir, module, export),
        Runtime::Python => resolve_python(graph, &h.base_dir, module, export),
        Runtime::Ruby => resolve_file_fn(
            graph,
            &format!("{}.rb", join_norm(&h.base_dir, module)),
            export,
        ),
        Runtime::Other => None,
        Runtime::Unknown => resolve_node(graph, &h.base_dir, module, export)
            .or_else(|| resolve_python(graph, &h.base_dir, module, export))
            .or_else(|| {
                resolve_file_fn(
                    graph,
                    &format!("{}.rb", join_norm(&h.base_dir, module)),
                    export,
                )
            }),
    }
}

/// Node: handler "src/handlers/users.create" -> module path + export; try
/// every JS/TS extension and /index.* under the module path.
fn resolve_node(graph: &GigaGraph, base: &str, module: &str, export: &str) -> Option<u32> {
    let stem = join_norm(base, module);
    for cand in NODE_EXTS
        .iter()
        .map(|e| format!("{stem}{e}"))
        .chain(NODE_EXTS.iter().map(|e| format!("{stem}/index{e}")))
    {
        if let Some(id) = resolve_file_fn(graph, &cand, export) {
            return Some(id);
        }
    }
    None
}

/// Python: "app.lambda_handler" and dotted "src.handlers.app.main" both
/// reduce to module-with-dots-as-slashes + .py; the slash-style serverless
/// form ("src/handlers/app.main") is the literal-path attempt.
fn resolve_python(graph: &GigaGraph, base: &str, module: &str, export: &str) -> Option<u32> {
    for m in [module.to_string(), module.replace('.', "/")] {
        let cand = format!("{}.py", join_norm(base, &m));
        if let Some(id) = resolve_file_fn(graph, &cand, export) {
            return Some(id);
        }
    }
    None
}

fn resolve_file_fn(graph: &GigaGraph, cand: &str, func: &str) -> Option<u32> {
    let file_id = find_file(graph, cand)?;
    // YAML pseudo-functions share names like "functions"/"Resources" across
    // the index, hence the file_id + !is_toplevel gate; unique hit only.
    let ids = graph.name_index.get(func)?;
    let hits: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|&id| {
            let f = &graph.functions[id as usize];
            f.file_id == file_id && !f.is_toplevel
        })
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Exact path first, then unique `/`-suffix match anywhere in the tree.
fn find_file(graph: &GigaGraph, cand: &str) -> Option<u32> {
    if let Some(&id) = graph.path_index.get(cand) {
        return Some(id);
    }
    let suffix = format!("/{cand}");
    let mut found: Option<u32> = None;
    for f in &graph.files {
        if f.path.ends_with(&suffix) {
            if found.is_some() {
                return None; // ambiguous
            }
            found = Some(f.id);
        }
    }
    found
}

// -------------------------------------------------------------- path utils

fn parent_dir(p: &str) -> &str {
    p.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Join + normalize `.`/`..`, `/`-separated, no leading slash.
fn join_norm(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}
