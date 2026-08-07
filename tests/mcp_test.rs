//! Protocol smoke test: spawn the real binary, speak MCP over stdio.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn rpc(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    msg: Value,
) -> Value {
    writeln!(stdin, "{msg}").expect("write");
    stdin.flush().expect("flush");
    let mut line = String::new();
    reader.read_line(&mut line).expect("read");
    serde_json::from_str(&line).expect("parse response")
}

#[test]
fn mcp_handshake_and_tools() {
    let root = format!("{}/tests/fixtures/mcp", env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_gigagraph"))
        .args(["serve", "--root", &root])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    // initialize
    let resp = rpc(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }}),
    );
    assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(resp["result"]["serverInfo"]["name"], "gigagraph");

    // Post-handshake sync: the handshake alone (no tool call) must leave a
    // persisted index behind. Background thread -> poll briefly.
    let index_file = std::path::Path::new(&root).join(".gigagraph/index.bin");
    let _ = std::fs::remove_file(&index_file);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !index_file.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        index_file.exists(),
        "initialize did not trigger a background index sync"
    );

    // initialized notification (no response expected)
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    stdin.flush().unwrap();

    // tools/list
    let resp = rpc(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "index_project",
        "search_functions",
        "get_function",
        "get_callers",
        "get_callees",
        "find_similar",
        "call_path",
        "file_overview",
        "list_packages",
        "index_stats",
        "blast_radius",
        "affected_tests",
        "list_endpoints",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // tools/call: search triggers lazy index build of the fixture.
    let resp = rpc(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "search_functions",
            "arguments": {"query": "hello"}
        }}),
    );
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("shoutHello"),
        "search result missing shoutHello: {text}"
    );

    // tools/call: callers via the graph.
    let resp = rpc(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": "get_callers",
            "arguments": {"function": "hello"}
        }}),
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("shoutHello"),
        "hello's caller should be shoutHello: {text}"
    );

    // unknown method -> JSON-RPC error
    let resp = rpc(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc": "2.0", "id": 5, "method": "bogus/method"}),
    );
    assert_eq!(resp["error"]["code"], -32601);

    drop(stdin);
    let _ = child.wait();
}
