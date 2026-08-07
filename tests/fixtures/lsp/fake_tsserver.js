#!/usr/bin/env node
// Fake tsserver for gigagraph's LSP enrichment tests.
//
// Speaks tsserver's real wire shape: requests arrive as single-line JSON on
// stdin; responses/events go out framed as
//   Content-Length: N\r\n\r\n<json>\n
// where N includes the trailing newline (exactly what TypeScript's
// session.ts `formatMessage` produces).
//
// `definition` requests are answered from an answers.json sitting NEXT TO
// this script, keyed "<basename>:<line>:<offset>" with values
// { "file": "<path relative to the project root, or absolute>", "line": N }.
// The project root is assumed three directories up (this script is placed at
// node_modules/typescript/lib/tsserver.js in test fixtures). Missing keys get
// success:false — like the real server on a position with no definition.
"use strict";
const fs = require("fs");
const path = require("path");
const readline = require("readline");

let answers = {};
try {
  answers = JSON.parse(
    fs.readFileSync(path.join(__dirname, "answers.json"), "utf8")
  );
} catch (e) {
  // No answers file: every definition request is unsuccessful.
}

function send(msg) {
  const body = JSON.stringify(msg) + "\n";
  process.stdout.write(
    `Content-Length: ${Buffer.byteLength(body, "utf8")}\r\n\r\n${body}`
  );
}

// The real server emits unsolicited events; clients must skip them.
send({
  seq: 0,
  type: "event",
  event: "typingsInstallerPid",
  body: { pid: process.pid },
});

const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on("line", (line) => {
  line = line.trim();
  if (!line) return;
  let req;
  try {
    req = JSON.parse(line);
  } catch (e) {
    return;
  }
  if (req.command === "open") {
    // No response for `open` — but events fly, like the real thing.
    send({
      seq: 0,
      type: "event",
      event: "projectLoadingStart",
      body: { projectName: "fake", reason: "open" },
    });
    send({
      seq: 0,
      type: "event",
      event: "projectLoadingFinish",
      body: { projectName: "fake" },
    });
    return;
  }
  if (req.command === "exit") {
    process.exit(0);
  }
  if (req.command === "definition" || req.command === "definitionAndBoundSpan") {
    const a = req.arguments || {};
    const key = `${path.basename(a.file || "")}:${a.line}:${a.offset}`;
    const hit = answers[key];
    if (hit) {
      const root = path.resolve(__dirname, "..", "..", "..");
      const abs = path.resolve(root, hit.file); // absolute stays absolute
      send({
        seq: 0,
        type: "response",
        command: req.command,
        request_seq: req.seq,
        success: true,
        body: [
          {
            file: abs,
            start: { line: hit.line, offset: hit.offset || 1 },
            end: { line: hit.line, offset: (hit.offset || 1) + 1 },
          },
        ],
      });
    } else {
      send({
        seq: 0,
        type: "response",
        command: req.command,
        request_seq: req.seq,
        success: false,
        message: "Could not find a definition.",
      });
    }
    return;
  }
  send({
    seq: 0,
    type: "response",
    command: req.command,
    request_seq: req.seq,
    success: false,
    message: "unhandled command",
  });
});
rl.on("close", () => process.exit(0));
