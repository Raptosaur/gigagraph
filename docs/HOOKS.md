# Claude Code integration: session-start sync + touch memory

## Session-start index sync

The MCP server syncs itself: the `initialize` handshake spawns a background
index build/refresh, so the first tool call answers from a warm cache. A
`SessionStart` hook is a belt-and-braces addition for sessions that begin
with reads or edits rather than MCP calls — the index (and its incremental
extraction cache) is fresh before anyone asks:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "gigagraph index \"$CLAUDE_PROJECT_DIR\" >/dev/null 2>&1 || true",
            "async": true,
            "statusMessage": "Syncing gigagraph index"
          }
        ]
      }
    ]
  }
}
```

Notes:

- `gigagraph index` is incremental: unchanged files are served from the
  extraction cache, so a warm re-sync on a medium repo is tens of
  milliseconds, not a full parse.
- `async: true` keeps session start unblocked; output is discarded so session
  context is not polluted with stats; `|| true` keeps a missing binary from
  failing session start.
- Use an absolute binary path (e.g. `~/.cargo/bin/gigagraph` from
  `cargo install --path .`) if gigagraph is not on the hook's `PATH`.

## Touch memory: agent-recorded, not hook-recorded

gigagraph keeps a persistent ring of recent edits — the *touch memory* — in
`<root>/.gigagraph/touches.jsonl`, read with the `recent_touches` MCP tool or
`gigagraph touches`.

There is deliberately **no automatic edit-logging hook**. A mechanical hook
records *what* changed but never *why* — and the WHY is the entire value of
the ring over `git log`. Instead, the MCP handshake `instructions` hold
agents to a touch discipline:

- **before** modifying an unfamiliar file: call `recent_touches` for it;
- **after** every substantive edit: call `record_touch` with the files and a
  one-line rationale, as part of finishing the edit.

The ring is capped at **250 entries globally** and **10 entries per file**
(oldest extras dropped, atomically, under a lock — concurrent writers are
safe). `git log` remains the authoritative history; the ring adds rationale
and covers uncommitted work.

## CLI reference

```sh
gigagraph index [path] [--force]
gigagraph touch --root . --why "refactor lock handling" --agent claude src/touches.rs src/api.rs
gigagraph touches [--root .] [--file src/api.rs] [--limit 20]
```

Paths may be repo-relative or absolute; they are stored repo-relative.
