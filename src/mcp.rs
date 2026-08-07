//! Minimal MCP server over stdio (newline-delimited JSON-RPC 2.0).

use crate::api::AppState;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub fn serve(mut state: AppState) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("parse error: {e}") }
                });
                writeln!(out, "{resp}")?;
                out.flush()?;
                continue;
            }
        };
        let responses = match msg {
            // JSON-RPC batch: answer each request, keep notification slots
            // silent, reply as a batch. Empty batch is invalid per spec.
            Value::Array(msgs) => {
                if msgs.is_empty() {
                    vec![json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32600, "message": "empty batch" }
                    })]
                } else {
                    let batch: Vec<Value> = msgs
                        .into_iter()
                        .filter_map(|m| handle_msg(&mut state, m))
                        .collect();
                    if batch.is_empty() {
                        Vec::new() // all notifications
                    } else {
                        vec![Value::Array(batch)]
                    }
                }
            }
            m => handle_msg(&mut state, m).into_iter().collect(),
        };
        for response in responses {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Handle one JSON-RPC message; `None` for notifications.
fn handle_msg(state: &mut AppState, msg: Value) -> Option<Value> {
    // Notifications get no response.
    let id = msg.get("id").cloned()?;
    // Spec: id must be a string, number, or null.
    if !matches!(id, Value::String(_) | Value::Number(_) | Value::Null) {
        return Some(json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": { "code": -32600, "message": "invalid request id type" }
        }));
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let response = match method {
        "initialize" => {
            // Post-handshake sync: warm the index in the background so the
            // agent's first structural query answers from a fresh cache.
            state.warm_in_background();
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18");
            // Echo a version we actually support; otherwise offer our latest.
            let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
                requested
            } else {
                "2025-06-18"
            };
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "gigagraph",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": "Semantic code graph over this repo. Use these tools instead of grep/reading files for STRUCTURAL questions: who calls X (get_callers), what does X call (get_callees), where is X defined (search_functions), how do A and B connect (call_path), what is built like X (find_similar), what API surface exists and who calls it (list_endpoints/find_endpoint_callers — REST routes plus SOAP/XML-RPC/JSON-RPC operations, gRPC services, GraphQL/AppSync resolvers, and routes declared in IaC: CloudFormation/SAM templates, serverless.yml, Terraform, CDK — with Lambda handlers linked to the indexed functions they run), what code is dead/unused (unreferenced_functions), who calls a React Native module from JS (bridge_map). BEFORE modifying a function, compute its blast radius pre-emptively: blast_radius shows everything that transitively reaches it (callers, endpoints, cross-service HTTP/RPC consumers, bridge call sites) so you can judge a change's reach BEFORE making it, and affected_tests tells you which tests a change can dirty — run those first after editing. Answers carry signatures + exact file:line, so a follow-up file read is usually unnecessary. Prefer plain grep/file reads instead when: (1) hunting exact literal text — string constants, comments, log messages, config keys; (2) the code is in an unsupported language or generated/minified; (3) you need dynamic/reflective dispatch that static resolution can't see (edges labeled confidence:heuristic are strong leads, not proof — verify load-bearing conclusions); (4) you need file content itself, not structure. Index refreshes automatically on every call; results always reflect the current tree. TOUCH DISCIPLINE (important — there is NO automatic edit logging; the shared history is exactly what agents record): (a) BEFORE modifying any file you did not just create, call recent_touches for it — another agent may have changed it minutes ago for reasons git can't show yet; (b) AFTER every substantive edit (new feature, bug fix, refactor, config/behavior change — not typo-level tweaks), call record_touch with the edited files and a one-line WHY. Treat record_touch as part of finishing the edit, like saving the file: an edit you didn't record is invisible to every other agent and future session working in this repo. When several files change for one reason, record them in ONE call with that reason."
                }
            })
        }
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_definitions() }
        }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match state.dispatch(name, &args) {
                Ok(result) => {
                    let text = serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string());
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        }
                    })
                }
                Err(e) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("error: {e:#}") }],
                        "isError": true
                    }
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("method not found: {method}") }
        }),
    };
    Some(response)
}

fn tool_definitions() -> Value {
    let fn_ref = "Function reference: `fn:<id>`, a qualified name (`path::Type::name`), or an unambiguous simple name.";
    json!([
        {
            "name": "index_project",
            "description": "Index (or re-index) the project's source tree. Incremental: unchanged files are served from cache. Run after significant code changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "force": { "type": "boolean", "description": "Ignore the extraction cache and re-parse everything." }
                }
            }
        },
        {
            "name": "index_stats",
            "description": "Summary of the current index: file/function/call counts, resolution rates, per-language breakdown.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "search_functions",
            "description": "Find functions by name (exact, prefix, substring, or fuzzy). Returns ids usable with the other tools. Finds only function DEFINITIONS — for arbitrary literal text (strings, comments, variables, config), plain grep is better and cheaper.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "get_function",
            "description": format!("Full card for one function: location, signature, every call it makes (resolved to internal functions or external packages), the external packages it uses, and its callers. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": { "function": { "type": "string" } },
                "required": ["function"]
            }
        },
        {
            "name": "get_callers",
            "description": format!("Who calls this function, with call sites (file:line). Includes ambiguous matches, flagged. Unlike grep, returns only real resolved call sites (no comments/strings/shadowed names) — but dynamic dispatch, reflection, and callbacks passed by value can be missed; grep for the name if an exhaustive textual sweep matters. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["function"]
            }
        },
        {
            "name": "get_callees",
            "description": format!("Every call this function makes: internal callees, external package calls, and unresolved names. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["function"]
            }
        },
        {
            "name": "find_similar",
            "description": format!("Semantically similar functions via blended vectors: structural (callee names, identifiers, AST shape, control flow, transitive effects) + distilled static name/doc embeddings, so fetchUser and loadAccount can match on meaning as well as shape. Query by indexed function or by raw snippet. Not answerable with grep at all; use for duplicate-logic hunting, convention discovery, refactor targets. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string", "description": "Indexed function to match against." },
                    "snippet": { "type": "string", "description": "Raw code to vectorize instead of an indexed function." },
                    "language": { "type": "string", "description": "Snippet language (c, javascript, typescript, tsx, java, kotlin, swift). Required with `snippet`." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "call_path",
            "description": format!("Shortest call chain from one function to another over resolved internal edges (BFS). {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "max_depth": { "type": "integer" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "file_overview",
            "description": "One file's imports (classified internal/external) and every function defined in it. Accepts a relative path or unique path suffix.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "list_packages",
            "description": "External packages the codebase calls into, ranked by call-site count.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer" } }
            }
        },
        {
            "name": "list_endpoints",
            "description": "API surface this codebase publishes: REST routes (Express/Fastify, Flask/FastAPI, Laravel/Symfony, Spring, ASP.NET, gin/echo/net-http, Sinatra/Rails, axum) plus RPC-style operations — SOAP (JAX-WS, WCF/ASMX, spyne, PHP SoapServer), XML-RPC, JSON-RPC, and gRPC service registrations (service-level; individual proto methods are not indexed). HTTP paths are normalized so /users/:id and /users/{id} compare equal; RPC rows carry the operation name in the path field with kind soap/xml-rpc/json-rpc/grpc.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "method": { "type": "string", "description": "Filter: GET/POST/PUT/DELETE/PATCH/ANY (RPC rows are ANY)." },
                    "path": { "type": "string", "description": "Filter: substring of the raw or normalized path / operation name." },
                    "framework": { "type": "string" },
                    "kind": { "type": "string", "description": "Filter: http, soap, xml-rpc, json-rpc, grpc, or graphql." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "find_endpoint_callers",
            "description": "Who calls this API route? Give a path (any parameter syntax: /users/:id, /users/{id}) and optional method; returns matching endpoints with their statically-detected in-repo HTTP callers (fetch/axios/requests/Guzzle/HttpClient/...), each with file:line and match confidence. External clients are invisible.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "method": { "type": "string" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "get_endpoint",
            "description": "Full card for one endpoint (`ep:<id>` from list_endpoints): handler function, all matched client calls with confidence.",
            "inputSchema": {
                "type": "object",
                "properties": { "endpoint": { "type": "string" } },
                "required": ["endpoint"]
            }
        },
        {
            "name": "list_client_calls",
            "description": "Outbound HTTP calls the codebase makes (fetch, axios, requests/httpx, Guzzle, net/http, HttpClient, HTTParty), with the calling function and any matched endpoints. `unmatched: true` shows calls that hit no known in-repo endpoint.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "unmatched": { "type": "boolean" },
                    "library": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "unreferenced_endpoints",
            "description": "Endpoints with no statically-detected in-repo caller. NOT proof of dead code: external/mobile/cross-repo clients are invisible to static indexing.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer" } }
            }
        },
        {
            "name": "blast_radius",
            "description": format!("Pre-emptive change-impact analysis: BEFORE modifying a function (or file), see everything that can transitively reach it — direct and indirect callers by depth, API endpoints whose handlers are implicated, cross-service consumers via correlated HTTP/RPC client calls, React Native bridge call sites, and how many tests the change can dirty. Static closure: dynamic dispatch and external callers are invisible; `heuristic` rows carry at least one uncertain edge. Seed with `function` or `file`. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string", "description": "Seed function to analyze." },
                    "file": { "type": "string", "description": "Seed with every function in this file instead." },
                    "max_depth": { "type": "integer", "description": "Caller-chain depth to explore (default 10, max 50)." },
                    "limit": { "type": "integer", "description": "Max impacted functions listed (counts are always exact)." }
                }
            }
        },
        {
            "name": "affected_tests",
            "description": format!("Which tests can a change to this function/file dirty? Walks the blast radius and returns the tests (and test-file helpers) inside it, grouped by file with the shallowest call depth — the practical 'run these first after editing' list. Detection: test decorations (#[test], @Test, [Fact], pytest.mark), naming conventions (test_*, TestXxx), and test-file paths (tests/, __tests__, *.spec.ts, *_test.go). An empty result narrows the run; it does not prove no test is affected (dynamic dispatch, fixtures, and data-driven tests are invisible). Seed with `function` or `file`. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string", "description": "Seed function." },
                    "file": { "type": "string", "description": "Seed with every function in this file instead." },
                    "max_depth": { "type": "integer", "description": "Caller-chain depth (default 20, max 50)." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "visualize",
            "description": "Generate an interactive 3D map of the codebase as one self-contained HTML file (functions clustered by structural similarity, call-graph edges, API endpoints ringed, search + focus). Writes <root>/.gigagraph/map.html and returns its absolute path — tell the user to open that file in a browser.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "record_touch",
            "description": "Record that you just edited files, with a one-line rationale. Appends to a persistent ring of recent touches shared by all agents working on this repo (last 250 entries, max 10 per file). `git log` is the authoritative history — this ring adds the WHY and covers uncommitted work. Call after completing a substantive edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Edited files (repo-relative or absolute; stored repo-relative)." },
                    "why": { "type": "string", "description": "One-line rationale for the edit." },
                    "agent": { "type": "string", "description": "Who is recording (defaults to \"unknown\")." }
                },
                "required": ["files", "why"]
            }
        },
        {
            "name": "recent_touches",
            "description": "What was recently edited in this repo and why, newest first — as reported by agents and hooks. Check before modifying unfamiliar files. Not authoritative: `git log` is the real history; this ring adds rationale and covers uncommitted work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Only entries mentioning this file (repo-relative or unique suffix)." },
                    "limit": { "type": "integer", "description": "Max entries (default 10, max 50)." }
                }
            }
        },
        {
            "name": "bridge_map",
            "description": "React Native bridge map: native @ReactMethod (Java/Kotlin) and RCT_EXPORT_METHOD (ObjC) implementations correlated by name with JS NativeModules call sites. Answers 'who calls this native method from JS' across the language boundary that call resolution can't see. getName() aliases and Swift modules are not matched — see the response note.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Filter by native module class name or its JS alias." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "unreferenced_functions",
            "description": "Dead/unused-code review queue: functions whose NAME is referenced nowhere — no call site (even unresolved), no identifier reference (callbacks by value), no import binding. Decorated functions, endpoint handlers, and entry-point conventions (main, dunders, Java_* JNI, on*/handle* callbacks, tests) are auto-excluded; public/exported functions are demoted to a separate list since external callers are invisible. Cross-language dispatch (JNI, RN bridge, reflection) cannot be seen — treat results as candidates to review, never a delete list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "language": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        }
    ])
}
