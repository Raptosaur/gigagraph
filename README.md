# gigagraph

Semantic code-graph MCP server for coding agents. Indexes **what code means
structurally** — every function, where it's called, what it calls, which
packages it depends on — rather than embedding source text. Written in Rust
with tree-sitter parsing and rayon multithreading; built to chew through large
multi-language monorepos.

**Languages:** C, C++, C#, Bash, Go, Java, JavaScript, Kotlin, Objective-C,
PHP, Python, Ruby, Rust, Swift, TypeScript (+TSX) — plus schema languages:
SQL (tables, views, functions, and their dependency edges), Prisma (models +
relations), GraphQL SDL (types + type dependencies), and shallow YAML.

## What it builds

For every function in the tree:

- **Definition**: name, qualified name (`scope::Type::name`), file:lines,
  signature, containing type, param count
- **Call graph**: every call site, resolved heuristically to the target
  function (same file → import-directed → same package → same directory →
  global name match, with confidence + ambiguity reporting), plus reverse
  (caller) edges
- **Package edges**: calls that leave the project are attributed to external
  packages via import analysis (`express`, `java.util`, `stdio.h`,
  `Foundation`, `builtin:console`, …)
- **Semantic vector**: two always-on signals per function, blended at query
  time (0.6/0.4, fixed — no configuration):
  - *structural* (256 dims): feature-hashing the function's build — callee
    names, identifier bag, AST node-type histogram, control-flow shape
    (loops/branches/nesting), arity, size, external packages — enriched with
    verb-synonym buckets (`fetch`/`load`/`get` → one READ feature),
    camelCase/snake_case subwords, typed-local types, and depth-weighted
    *transitive effect* features (the helpers a function reaches and the
    external packages it ultimately touches, 3 calls deep) — IDF-weighted,
    L2-normalized;
  - *semantic* (64 dims): the function's identifier/callee/type/doc words
    embedded with a distilled static embedding model compiled into the binary
    (int8-quantized [potion-base-2M], ~2.2 MB, MIT — see `src/embed/NOTICE`);
    static table lookup + mean-pool, microseconds per function, no network,
    no runtime downloads.

  Similarity search is brute-force blended cosine over the in-memory
  matrices, parallelized and fully deterministic: two functions are similar
  when they're built the same way *and* named/documented with the same
  meaning.

[potion-base-2M]: https://huggingface.co/minishlab/potion-base-2M

- **API endpoint map**: routes the code publishes (Express/Koa/Fastify/Hono/
  restify, NestJS, Flask/FastAPI/Django, Laravel/Symfony/Slim, Spring,
  ASP.NET, gin/echo/chi/gorilla/net-http, Sinatra/Rails, axum/actix, Ktor)
  correlated with the outbound HTTP calls that hit them (fetch, XHR, jQuery,
  axios/got/ky/superagent, requests/httpx/aiohttp, Guzzle, net/http,
  HttpClient, HTTParty, Retrofit) — matched on normalized path templates
  (`/users/:id` ≡ `/users/{id}` ≡ `/users/{*}`) + method, with mount/group/
  controller prefix joining and the same high/heuristic confidence labeling
  as call resolution.
- **React Native bridge map**: JS `NativeModules` call sites correlated with
  native `@ReactMethod` (Java/Kotlin) and `RCT_EXPORT_METHOD` (ObjC)
  implementations — the cross-language edge static resolution can't see.
- **Dead-code review queue**: functions whose name is referenced nowhere (no
  call site even unresolved, no identifier reference, no import binding),
  with framework/entry-point/export suppression and honest caveats.
- **Touch memory**: a ring of recent edits with agent-supplied rationale
  (`record_touch`/`recent_touches`; the handshake instructs agents to record
  every substantive edit with its rationale — no mechanical hook, docs/HOOKS.md).
- **3D code map**: `gigagraph visualize` renders an offline WebGL map of the
  codebase, PCA-projected from the similarity vectors so structurally similar
  code clusters together.

Index and extraction cache persist under `<root>/.gigagraph/` (auto-gitignored).
Re-indexing is incremental: unchanged files (by content hash) skip parsing.
Every MCP tool call runs a stat-only staleness probe first and transparently
re-indexes changed files — answers always reflect the current tree, and an
unchanged tree costs zero file reads.

## Install

### Prebuilt binaries (recommended)

Grab the latest release for your platform from
[Releases](https://github.com/Raptosaur/gigagraph/releases). One-liners:

```sh
# macOS (Apple silicon)
curl -fsSL https://github.com/Raptosaur/gigagraph/releases/latest/download/gigagraph-aarch64-apple-darwin.tar.gz \
  | tar -xz && sudo mv gigagraph /usr/local/bin/

# macOS (Intel)
curl -fsSL https://github.com/Raptosaur/gigagraph/releases/latest/download/gigagraph-x86_64-apple-darwin.tar.gz \
  | tar -xz && sudo mv gigagraph /usr/local/bin/

# Linux (x86_64)
curl -fsSL https://github.com/Raptosaur/gigagraph/releases/latest/download/gigagraph-x86_64-unknown-linux-gnu.tar.gz \
  | tar -xz && sudo mv gigagraph /usr/local/bin/

# Linux (arm64)
curl -fsSL https://github.com/Raptosaur/gigagraph/releases/latest/download/gigagraph-aarch64-unknown-linux-gnu.tar.gz \
  | tar -xz && sudo mv gigagraph /usr/local/bin/
```

Windows: download `gigagraph-x86_64-pc-windows-msvc.zip` from
[Releases](https://github.com/Raptosaur/gigagraph/releases), unzip, and put
`gigagraph.exe` somewhere on your `PATH`.

Each artifact ships with a `.sha256` checksum file.

### Cargo

```sh
cargo install gigagraph                                        # from crates.io
cargo install --git https://github.com/Raptosaur/gigagraph     # from git
```

### From source

```sh
git clone https://github.com/Raptosaur/gigagraph && cd gigagraph
cargo build --release        # binary at target/release/gigagraph
```

## MCP registration

### Claude Code

With `gigagraph` on your `PATH`, register it once for all projects:

```sh
claude mcp add --scope user gigagraph -- gigagraph serve --root .
```

Or per project (`--scope project` writes a shareable `.mcp.json`). Manual
config equivalent:

```json
{
  "mcpServers": {
    "gigagraph": {
      "command": "gigagraph",
      "args": ["serve", "--root", "."]
    }
  }
}
```

`--root .` resolves against the directory the client launches the server in
(your project root for Claude Code), so one user-level registration serves
every project with its own index.

Any MCP client speaking stdio works the same way. The `initialize` handshake
kicks off a background index sync, so by the time the agent issues its first
structural query the cache is warm; every later tool call re-checks a
stat-only tree fingerprint and refreshes incrementally when files changed.
`index_project` remains available for an explicit `force` re-parse.

Optionally, a Claude Code `SessionStart` hook can pre-warm the index even
before the MCP handshake (useful when sessions start with file reads, not
tool calls) — see `docs/HOOKS.md`.

## Tools

| Tool | What it answers |
|---|---|
| `index_project` | (Re)index the tree. Incremental; `force` re-parses all. |
| `index_stats` | Files/functions/calls, resolution rates, per-language counts. |
| `search_functions` | Find functions by name (exact/prefix/substring/fuzzy). |
| `get_function` | Full card: location, signature, calls out (resolved), packages used, callers. |
| `get_callers` | Who calls this? Call sites with file:line, ambiguity flagged. |
| `get_callees` | Everything this calls: internal, external package, unresolved. |
| `find_similar` | Structurally similar functions — by indexed function or raw snippet. |
| `call_path` | Shortest call chain between two functions (BFS). |
| `file_overview` | One file's imports (classified) + functions. |
| `list_packages` | External packages ranked by call-site count. |
| `list_endpoints` | Published API surface: REST routes, SOAP/XML-RPC/JSON-RPC operations, gRPC services, GraphQL/AppSync resolvers, and IaC-declared routes (`kind` filter). |
| `find_endpoint_callers` | Who calls `POST /api/users/:id`? Endpoints + their in-repo HTTP callers. |
| `get_endpoint` | One endpoint's full card: handler, matched clients, confidence. |
| `list_client_calls` | Outbound HTTP/RPC calls, with `unmatched: true` filter. |
| `unreferenced_endpoints` | Endpoints no in-repo client hits (external callers invisible — not proof of dead code). |
| `unreferenced_functions` | Dead-code review queue; framework/entry-point conventions auto-excluded. |
| `blast_radius` | Pre-emptive change impact: transitive callers by depth, implicated endpoints, cross-service consumers via correlated HTTP/RPC calls, RN bridge sites, affected-test count. |
| `affected_tests` | Which tests can a change to this function/file dirty, grouped by file (the re-run unit). |
| `bridge_map` | React Native bridge: native methods ↔ JS `NativeModules` call sites. |
| `visualize` | Self-contained 3D HTML map of the codebase. |
| `record_touch` / `recent_touches` | Shared editing memory: what was changed and why, across agents. |

Function references accept `fn:<id>`, a qualified name, or an unambiguous
simple name.

## CLI (debugging)

```sh
gigagraph index .                              # build index, print stats
gigagraph query search_functions '{"query":"parse"}' --root .
gigagraph query find_similar '{"function":"fn:42"}' --root .
cargo run --example dump -- path/to/file.kt    # inspect a file's AST
```

## Architecture

```
src/
  lang/        one tree-sitter query + metadata per language (docs/QUERY_CONTRACT.md)
  extract.rs   generic query-driven extraction: functions, calls, imports, features
  graph.rs     id assignment, import classification, heuristic call resolution
  vector.rs    feature-hashed structural vectors + parallel blended cosine top-k
  verbs.rs     identifier word-splitting + verb-synonym bucketing
  embed.rs     compiled-in distilled static embeddings (src/embed/, ~2.2 MB)
  indexer.rs   parallel walk (gitignore-aware) -> cached extract -> graph -> vectors
  mcp.rs       stdio JSON-RPC MCP server
  api.rs       tool implementations
```

Parsing is per-file parallel (rayon); resolution is per-function parallel;
similarity search is chunk-parallel. Adding a language = one file with a
tree-sitter query following the capture contract, plus fixtures.

## Benchmarks

Apple M1 (8 cores), release build, cold = empty cache:

| Repo | Files | Functions | Call sites | Cold index | Warm index |
|---|---|---|---|---|---|
| vuejs/core (TS) | 527 | 5,315 | 56,474 | 392 ms | 213 ms |
| square/okhttp (Kotlin/Java) | 668 | 7,581 | 56,678 | 439 ms | — |
| BurntSushi/ripgrep (Rust) | 113 | 3,012 | 17,132 | 534 ms | — |
| pallets/flask (Python) | 83 | 1,512 | 3,895 | 395 ms | — |

Queries (`search_functions`, `get_callers`, `find_similar`, `call_path`)
answer in ~20 ms against the okhttp index, load included.

## Honesty about resolution

Cross-file resolution is heuristic (no full type inference), but it is
**dependency-injection-aware**: declared field/property types, typed
parameters, `x = new T()` locals, and `implements`/`extends` hierarchies are
captured per language, so `this.userService.getUser()` narrows to
`UserService`'s methods, and calls through an interface expand to its
implementations (single implementor resolves cleanly; several are honest
`heuristic` with `ambiguous_with` rivals; implementations outrank abstract
signatures). Explicit container registrations (Laravel `$app->bind(A::class,
B::class)`) name THE implementation and pre-empt the hierarchy fan-out.
Remaining method calls through untyped values (`obj.save()`) resolve by
method name + receiver hints and are labeled `confidence: "heuristic"`.
Agents should treat `high` as trustworthy and `heuristic` as a strong lead.
