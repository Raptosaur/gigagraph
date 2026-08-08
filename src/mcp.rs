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
                    "instructions": "Semantic code graph over this repo. Answers carry signatures and exact file:line, so a follow-up file read is usually unnecessary.\n\nROUTE BY QUESTION:\n- Where is X defined? search_functions\n- Who calls X / what does X call? get_callers / get_callees\n- How does A reach B? call_path\n- What breaks if I change X? blast_radius (call BEFORE editing)\n- What must I re-run after editing? affected_tests, then test_command for the command\n- What tests exist / is X tested? list_tests\n- What routes do we publish, and who calls them? list_endpoints / find_endpoint_callers\n- What outbound HTTP do we make? list_client_calls\n- What else is built like X? find_similar\n- What is in this file or package? file_overview (pass `dir` for a whole package)\n- What looks dead? unreferenced_functions / unreferenced_endpoints\n- Who calls this native module from JS? bridge_map\n- Why is my code missing from an answer? extract_file, then supported_languages, then index_stats.skipped_paths\n\nUSE GREP INSTEAD when you need literal text (strings, comments, config keys), file content rather than structure, an unsupported or generated/minified language, or an exhaustive sweep including dynamic dispatch. Edges marked confidence:heuristic are strong leads, not proof — verify anything load-bearing.\n\nThe index refreshes itself on every call; you do not need index_project.\n\nTOUCH DISCIPLINE — nothing logs edits automatically, so this shared history is exactly what agents record:\n- BEFORE editing a file you did not just create, call recent_touches for it.\n- AFTER a substantive edit, call record_touch with the files and a one-line WHY. Treat it as part of saving the file; an unrecorded edit is invisible to every other agent. Group files changed for one reason into one call."
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
    let fn_ref = "Accepts `fn:<id>`, a qualified name (`path::Type::name`), or an unambiguous simple name.";
    json!([
        {
            "name": "search_functions",
            "description": "Where is this function defined? Name search (exact, prefix, substring, fuzzy) over every function in the repo. Start here when you have a name and need the code. Returns ids the other tools take. Definitions only — for literal text (strings, comments, config keys) use grep.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Function name or part of one." },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "get_function",
            "description": format!("What does this function do and how does it connect? One card: location, signature, what it calls, what packages it uses, who calls it. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": { "function": { "type": "string" } },
                "required": ["function"]
            }
        },
        {
            "name": "get_callers",
            "description": format!("Who calls this function? Resolved call sites with file:line — no comment/string/shadowed-name noise that grep returns. Misses dynamic dispatch and callbacks passed by value; grep the name too when you need an exhaustive sweep. {fn_ref}"),
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
            "description": format!("What does this function call? Internal callees, external package calls, and names that stayed unresolved. {fn_ref}"),
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
            "name": "call_path",
            "description": format!("How does A reach B? Shortest call chain between two functions. {fn_ref}"),
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
            "name": "blast_radius",
            "description": format!("What breaks if I change this? Call BEFORE editing. Everything that transitively reaches the function or file: callers by depth, endpoints whose handlers are implicated, cross-service HTTP/RPC consumers, React Native bridge sites, affected-test count. Static: dynamic dispatch and out-of-repo callers are invisible, and `heuristic` rows rest on at least one uncertain edge. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string", "description": "Seed function." },
                    "file": { "type": "string", "description": "Seed with every function in this file instead." },
                    "max_depth": { "type": "integer", "description": "Caller-chain depth (default 10, max 50)." },
                    "limit": { "type": "integer", "description": "Max functions listed; counts stay exact." }
                }
            }
        },
        {
            "name": "affected_tests",
            "description": format!("Which tests must I re-run after this edit? The tests inside the change's blast radius, grouped by file (the file is the re-run unit). Pair with test_command to get the command. Empty narrows the run — it does not prove nothing is affected. {fn_ref}"),
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
            "name": "list_tests",
            "description": "What tests exist? The repo's standing inventory: every named case with its runner and suite, grouped by file. Use for \"is X tested\", \"what framework does this repo use\", \"show me the tests for this module\". (For \"what should I re-run after my edit\", use affected_tests.) Covers annotations, runner naming conventions, and block styles (describe/it, gtest TEST, Catch2 TEST_CASE, bats @test) across every supported language.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Substring of the file path — a file or a directory." },
                    "name": { "type": "string", "description": "Substring of the case name or its suite." },
                    "framework": { "type": "string", "description": "pytest, unittest, jest, vitest, mocha, node-test, go-test, junit, xunit, nunit, mstest, rust-test, rspec, minitest, phpunit, gtest, catch2, xctest, swift-testing, bats, shunit2, c-test." },
                    "language": { "type": "string", "description": "python, typescript, go, ..." },
                    "include_hooks": { "type": "boolean", "description": "Also return suites and setup/teardown/fixtures (default false)." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "test_command",
            "description": "How do I run this test? Turns a test file or case name into the shell command for its runner, using the project's own build tooling (gradle vs maven, swift test vs xcodebuild, bundler or not). Returns both the single-case command and the whole-file one. Use right after list_tests or affected_tests instead of guessing the invocation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Test file (substring of its path)." },
                    "name": { "type": "string", "description": "Case name; omit to get the whole-file command." },
                    "framework": { "type": "string", "description": "Narrow when one file holds more than one runner." }
                }
            }
        },
        {
            "name": "list_endpoints",
            "description": "What API surface does this codebase publish? REST routes plus SOAP/XML-RPC/JSON-RPC operations, gRPC services, GraphQL resolvers, and routes declared in IaC (CloudFormation/SAM, serverless.yml, Terraform, CDK) with their Lambda handlers linked. Paths are normalized, so /users/:id and /users/{id} compare equal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "method": { "type": "string", "description": "GET/POST/PUT/DELETE/PATCH/ANY (RPC rows are ANY)." },
                    "path": { "type": "string", "description": "Substring of the raw or normalized path / operation name." },
                    "framework": { "type": "string" },
                    "kind": { "type": "string", "description": "http, soap, xml-rpc, json-rpc, grpc, graphql." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "find_endpoint_callers",
            "description": "Who calls this API route? Give a path in any parameter syntax (/users/:id, /users/{id}); returns matching endpoints and the in-repo code that calls them (fetch/axios/requests/Guzzle/HttpClient/...), with file:line. Callers outside this repo are invisible.",
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
            "description": "Full card for one endpoint (`ep:<id>` from list_endpoints): its handler function and every matched client call.",
            "inputSchema": {
                "type": "object",
                "properties": { "endpoint": { "type": "string" } },
                "required": ["endpoint"]
            }
        },
        {
            "name": "list_client_calls",
            "description": "What outbound HTTP calls does this codebase make? fetch, axios, requests/httpx, Guzzle, net/http, HttpClient, HTTParty — with the calling function and any endpoint they were matched to. `unmatched: true` shows calls hitting no known in-repo route.",
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
            "name": "find_similar",
            "description": format!("What else is built like this? Ranks functions by structure (callees, AST shape, control flow, transitive effects) blended with name/doc meaning, so fetchUser and loadAccount match. Grep cannot answer this. Use for duplicate logic, convention discovery, refactor targets, and \"show me the existing pattern before I write a new one\". Query by indexed function or raw snippet. {fn_ref}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function": { "type": "string", "description": "Indexed function to match against." },
                    "snippet": { "type": "string", "description": "Raw code to vectorize instead." },
                    "language": { "type": "string", "description": "Snippet language. Required with `snippet`." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "file_overview",
            "description": "What is in this file (or this directory)? Imports, classified internal vs external, plus every function defined. Pass `dir` instead of `path` to get a whole package in one call rather than a round trip per file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "One file: relative path or unique suffix." },
                    "dir": { "type": "string", "description": "Every indexed file under this directory." },
                    "limit": { "type": "integer", "description": "Max files for `dir` (default 50)." }
                }
            }
        },
        {
            "name": "extract_file",
            "description": "Why isn't my code showing up? Raw parser output for one file — functions, decorations/annotations, string-literal call arguments, imports, class hierarchy — before any resolution. Use when a function, route, or test you can see in the source is missing from another tool's answer: this shows whether extraction saw it at all. Works on files that are not indexed.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Relative, absolute, or unique-suffix path." } },
                "required": ["path"]
            }
        },
        {
            "name": "supported_languages",
            "description": "Can this server read that file? Lists every language and the extensions it claims; pass `path` to ask about one file specifically. Check here before concluding code is missing — an unsupported extension is invisible to every other tool.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Ask whether this specific file would be indexed." } }
            }
        },
        {
            "name": "unreferenced_functions",
            "description": "What code looks dead? Functions whose name appears nowhere else — no call, no identifier reference, no import binding. Entry points, decorated functions, endpoint handlers and tests are excluded; exported functions are demoted to a separate list because callers outside the repo are invisible. A review queue, never a delete list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "language": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "unreferenced_endpoints",
            "description": "Which routes have no in-repo caller? Same caveat as dead code, only stronger: browsers, mobile apps and other services are invisible to static indexing, so this is not proof a route is unused.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer" } }
            }
        },
        {
            "name": "bridge_map",
            "description": "Who calls this native module from JS? React Native bridge: native @ReactMethod (Java/Kotlin) and RCT_EXPORT_METHOD (ObjC) implementations matched by name to NativeModules call sites — the language boundary ordinary call resolution cannot cross.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Native module class name or its JS alias." },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "list_packages",
            "description": "What does this codebase depend on in practice? External packages ranked by how many call sites actually use them — usage, not the manifest.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer" } }
            }
        },
        {
            "name": "recent_touches",
            "description": "Has someone just changed this file, and why? Recent edits reported by agents and hooks, newest first. Check before editing a file you did not just write — another agent may have changed it minutes ago for reasons `git log` cannot show yet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Only entries mentioning this file." },
                    "limit": { "type": "integer", "description": "Default 10, max 50." }
                }
            }
        },
        {
            "name": "record_touch",
            "description": "Log what you just changed and why. Call after finishing a substantive edit — treat it as part of saving the file. Nothing logs edits automatically, so an unrecorded edit is invisible to every other agent working here. Group files that changed for one reason into a single call.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Edited files." },
                    "why": { "type": "string", "description": "One line: why, not what." },
                    "agent": { "type": "string", "description": "Who is recording." }
                },
                "required": ["files", "why"]
            }
        },
        {
            "name": "index_stats",
            "description": "How much of this repo does the index actually see? File/function/call counts, per-language breakdown, call-resolution rates, endpoint handler links, and the files that were skipped. Read `skipped_paths` when an answer looks incomplete — a skipped file is invisible everywhere.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "index_project",
            "description": "Rebuild the index. Rarely needed: every tool refreshes automatically when files change. Use `force: true` after upgrading the server or when results look stale despite an unchanged tree.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "force": { "type": "boolean", "description": "Ignore the extraction cache and re-parse everything." }
                }
            }
        },
        {
            "name": "visualize",
            "description": "Build an interactive 3D map of the codebase as one self-contained HTML file: functions clustered by similarity, call edges, endpoints ringed, client-call arcs, search and focus. Writes <root>/.gigagraph/map.html and returns the path — tell the user to open it in a browser.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}
