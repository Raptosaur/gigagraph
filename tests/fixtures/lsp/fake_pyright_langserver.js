#!/usr/bin/env node
// Fake pyright-langserver for gigagraph's LSP enrichment tests.
//
// Unlike the fake tsserver, this speaks real LSP: JSON-RPC 2.0 with
//   Content-Length: N\r\n\r\n<json>
// framing in BOTH directions, and it enforces the lifecycle the client must
// drive: initialize -> initialized -> didOpen before any definition on that
// document. Violations hard-exit(1) so the client's tests fail loudly.
//
// It also reproduces pyright's chatty side so the client proves it copes:
// unsolicited log/progress notifications, and a server->client
// workspace/configuration REQUEST (string id) fired on the first didOpen —
// definition answers are withheld until the client replies to it, exactly
// the stall a client that ignores server requests would hit with the real
// thing.
//
// `textDocument/definition` is answered from an answers.json sitting NEXT TO
// this script, keyed "<basename>:<line>:<character>" in gigagraph's 1-BASED
// coordinates (the wire is 0-based; the lookup adds 1 back, so a client that
// forgot the conversion misses every key). Values:
//   { "file": "<root-relative or absolute>", "line": N,        // 1-based
//     "shape": "location" | "locations" | "locationLink" }     // default location
// Missing key -> result: null, like the real server on a blank position.
// The project root is assumed two directories up (this script is installed at
// node_modules/pyright/langserver.index.js in test fixtures).
"use strict";
const fs = require("fs");
const path = require("path");
const { pathToFileURL, fileURLToPath } = require("url");

let answers = {};
try {
  answers = JSON.parse(
    fs.readFileSync(path.join(__dirname, "answers.json"), "utf8")
  );
} catch (e) {
  // No answers file: every definition request yields null.
}

const root = path.resolve(__dirname, "..", "..");

function send(msg) {
  const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...msg }), "utf8");
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

let initialized = false;
let initializedNote = false;
let sentConfigRequest = false;
let configAcked = false;
const opened = new Set();
const pendingDefinitions = [];

// The real pyright logs eagerly, before the client asks anything.
send({
  method: "window/logMessage",
  params: { type: 3, message: "fake pyright starting" },
});

function answerDefinition(msg) {
  const p = msg.params;
  const uri = p.textDocument.uri;
  if (!opened.has(uri)) {
    send({
      id: msg.id,
      error: { code: -32602, message: "definition before didOpen" },
    });
    return;
  }
  let base = "";
  try {
    base = path.basename(fileURLToPath(uri));
  } catch (e) {
    // Unparseable URI: fall through to a miss.
  }
  const key = `${base}:${p.position.line + 1}:${p.position.character + 1}`;
  const hit = answers[key];
  if (!hit) {
    send({ id: msg.id, result: null });
    return;
  }
  const abs = path.resolve(root, hit.file); // absolute stays absolute
  const defUri = pathToFileURL(abs).toString();
  const line0 = hit.line - 1;
  const range = {
    start: { line: line0, character: 4 },
    end: { line: line0, character: 5 },
  };
  let result;
  if (hit.shape === "locations") {
    result = [{ uri: defUri, range }];
  } else if (hit.shape === "locationLink") {
    result = [
      {
        targetUri: defUri,
        targetRange: {
          start: { line: line0, character: 0 },
          end: { line: line0 + 1, character: 0 },
        },
        targetSelectionRange: range,
      },
    ];
  } else {
    result = { uri: defUri, range };
  }
  send({ id: msg.id, result });
}

function flushDefinitions() {
  while (pendingDefinitions.length) {
    answerDefinition(pendingDefinitions.shift());
  }
}

function handle(msg) {
  if (msg.method === undefined && msg.id !== undefined) {
    // A response FROM the client — to our workspace/configuration request.
    if (msg.id === "cfg-1") {
      configAcked = true;
      flushDefinitions();
    }
    return;
  }
  switch (msg.method) {
    case "initialize":
      initialized = true;
      // Progress noise mid-handshake; the client must skip notifications
      // and match its response by id, not by arrival order.
      send({
        method: "$/progress",
        params: { token: "idx", value: { kind: "begin", title: "analyzing" } },
      });
      send({ id: msg.id, result: { capabilities: { definitionProvider: true } } });
      send({
        method: "$/progress",
        params: { token: "idx", value: { kind: "end" } },
      });
      return;
    case "initialized":
      if (!initialized) process.exit(1);
      initializedNote = true;
      return;
    case "textDocument/didOpen":
      if (!initialized || !initializedNote) process.exit(1);
      opened.add(msg.params.textDocument.uri);
      if (!sentConfigRequest) {
        sentConfigRequest = true;
        send({
          id: "cfg-1",
          method: "workspace/configuration",
          params: { items: [{ section: "python" }] },
        });
      }
      return;
    case "textDocument/definition":
      if (!initialized || !initializedNote) process.exit(1);
      pendingDefinitions.push(msg);
      if (configAcked) flushDefinitions();
      return;
    case "shutdown":
      send({ id: msg.id, result: null });
      return;
    case "exit":
      process.exit(0);
      return;
    default:
      if (msg.id !== undefined) {
        send({ id: msg.id, error: { code: -32601, message: "unhandled" } });
      }
  }
}

let buf = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  for (;;) {
    const headerEnd = buf.indexOf("\r\n\r\n");
    if (headerEnd < 0) return;
    const m = /Content-Length:\s*(\d+)/i.exec(buf.slice(0, headerEnd).toString("utf8"));
    if (!m) process.exit(1); // client must frame its side too
    const len = parseInt(m[1], 10);
    const start = headerEnd + 4;
    if (buf.length < start + len) return;
    const body = buf.slice(start, start + len).toString("utf8");
    buf = buf.slice(start + len);
    let msg;
    try {
      msg = JSON.parse(body);
    } catch (e) {
      continue;
    }
    handle(msg);
  }
});
process.stdin.on("end", () => process.exit(0));
