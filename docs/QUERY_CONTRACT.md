# Language extractor contract

Each language lives in `src/lang/<language>.rs` and exposes `pub fn spec() -> LangSpec`.
The generic extractor (`src/extract.rs`) consumes a single tree-sitter query per
language via **standard capture names**. No per-language Rust code is needed —
only the query string and the metadata lists on `LangSpec`.

## Capture names

| Capture | Meaning |
|---|---|
| `@func.def` | Whole function/method definition node. Its range becomes the function span; call sites inside it get attributed to it. |
| `@func.name` | Name node (same match as `@func.def`, required). |
| `@func.params` | Parameter-list node. Param count = named children (comments excluded). |
| `@func.body` | Optional: a value node (e.g. arrow function) the extractor probes for a `parameters`/`parameter` field when `@func.params` can't be captured directly. |
| `@call` | Whole call node. |
| `@call.name` | Callee simple-name node (same match, required). |
| `@call.recv` | Optional receiver node (`obj` in `obj.m()`). Discarded if text contains `(`/newline or exceeds 48 chars. |
| `@call.args` | Optional argument-list node. Arg count = named children. |
| `@call.typearg` | Optional generic type argument at the call site (`AddScoped<IStore, DbStore>()`). **Repeatable**: the extractor collects every `@call.typearg` capture for the call — within one match and across quantifier-split matches of the same pattern — in order, into `RawCall.type_args` (duplicate texts collapse). Capture only simple-name nodes (bare identifiers); qualified/generic/predefined type args should simply not match. Fuels .NET DI-binding detection (`AddScoped`/`AddSingleton`/`AddTransient` with exactly two type args → `di_bindings`) in `src/graph.rs`. |
| `@import` | Whole import/include node. Multiple query patterns may capture the same `@import` node; their `path`/`name` captures are merged by node identity. |
| `@import.path` | Module path / header node. Surrounding quotes/angles are stripped. |
| `@import.path.system` | Same, but marks a system include (C `<...>`). |
| `@import.name` | Local name(s) bound by the import (aliases, named imports). Repeatable. |
| `@package.name` | Declared package/module name node (Java/Kotlin `package` decl). First match wins. |
| `@deco` | Whole decorator/annotation/attribute node (`@app.route(...)`, `@GetMapping(...)`, `#[Route(...)]`). Deduped by byte range. |
| `@deco.name` | Decoration name node (same match, required). Dotted text like `app.route` is fine. |
| `@deco.args` | Decoration argument-list node; harvested into `ArgLit`s like call args. |
| `@deco.type` | Decorated type's name node for class-level decorations; routed to `ExtractedFile.type_decorations` instead of a function. |

Decoration→function association: a `@func.def` captured in the same match wins
(Java/C#/PHP annotations nest inside the definition node — write the pattern
that way); otherwise the nearest function starting after the decoration ends
(Python decorators). Decorations feed endpoint detection (`src/endpoints.rs`),
which holds all framework interpretation — queries stay framework-agnostic.

## Argument literals

`LangSpec.string_kinds` lists the grammar's string-literal node kinds. The
extractor walks every `@call.args` / `@deco.args` node and distills up to 10
`ArgLit`s per call: string literals (quotes stripped) and identifier-ish args,
with kwarg/object keys captured one level down (`methods=["POST"]`,
`{ method: "PUT" }`). Single-child wrapper nodes are unwrapped automatically.
One targeted depth-3 reach: a **multi-member array** value of an object member
(`{ method: ['GET', 'POST'] }`) emits each Ident/Str member keyed by the
member's key, all at the argument's index — while a single-member array keeps
its historical unwrapped-unkeyed shape. No query changes are needed for any of
this — only `string_kinds` metadata.

**`@Module` deep harvest**: NestJS provider objects
(`@Module({providers: [{provide: X, useClass: Y}]})`) sit at depth 3, below
the generic walk. For a decoration named exactly `Module`, the extractor runs
a targeted secondary harvest of the `@deco.args` node: each provider object's
`provide`/`useClass` values are appended as a synthetic keyed `ArgLit` pair
sharing an `index` (the provider ordinal, how `src/graph.rs` pairs them back
into `di_bindings`). These pairs are appended OUTSIDE the 10-lit cap so a long
providers array cannot evict real signal; positional consumers ignore them
(keys no detector matches). No query changes needed — it reuses `@deco.args`.

Notes:
- Patterns match **anywhere** in the tree (exported/nested/annotated wrappers
  don't need their own patterns).
- Duplicate `@func.def` matches on the same byte range are deduped; write
  overlapping patterns freely (e.g. C pointer-declarator nesting levels).
- `#eq?` / `#match?` predicates work (the extractor passes source bytes).
- A query referencing a node kind or field that doesn't exist in the grammar
  fails **at registry init** — `tests/registry_test.rs` catches this.

## Type information (DI-aware resolution)

Three optional capture families feed the receiver-type resolution rung in
`src/graph.rs` (`this.userService.getUser()` narrows to `UserService`'s
methods, expanded through interface→implementation edges). Call-side queries
need **no changes** — `@call.recv` already carries the dotted receiver text.
Bare two-segment receivers (`s.store.Save()` where `s` is a typed local —
e.g. a Go method receiver) resolve through local→type→field→type, capped at
Heuristic confidence; uppercase-initial bases (`System.out`, `Acme.Store`)
and self-qualified multi-hop (`this.a.b`) stay excluded.

| Capture | Meaning |
|---|---|
| `@field.name` + `@field.type` | Typed field/property on a class-like type (same match). Owner = nearest `type_kinds` ancestor of the name node, unless `@field.owner` is captured in the same match (Go structs, where the owner is a sibling `type_spec`, not an ancestor). Includes constructor-parameter properties (TS `constructor(private x: T)`, PHP promoted props, Kotlin class params). |
| `@local.name` + `@local.type` | Typed binding in a function body: parameters (`svc: UserService`), annotated locals (`let x: T`), and constructed locals (`x = new T()`, `let x = T::new()`). Attributed to the innermost containing function; toplevel bindings are dropped. |
| `@hier.type` + `@hier.base` | One inheritance/implementation edge per match: declaring type → base class or interface. Inverted at graph build into `type_bindings` (base → derived) for interface→impl expansion. |

Type nodes must be simple names: the extractor's `clean_type` rejects
generics/unions/tuples wholesale and keeps only the last qualified segment
(`cdk.App` → `App`, `App\Models\User` → `User`). Python `self.x = x` fields
may capture the **parameter name** in type position; the graph build
substitutes the `__init__` parameter's declared type (one-hop, like
`substitute_consts`).

## LangSpec metadata

- `identifier_kinds`: node kinds harvested as `id:<text>` features (semantic
  similarity vectors).
- `type_kinds`: `(node_kind, name_field)` pairs; nearest ancestor of a
  `@func.def` matching one becomes `containing_type` (methods know their
  class/struct/object).
- `loop_kinds` / `branch_kinds`: control-flow feature counting.
- `import_style`: `PathLike` (JS/C), `DottedPackage` (Java/Kotlin), `Module`
  (Swift) — drives external-package classification in `src/graph.rs`.
- `builtin_receivers`: receivers treated as stdlib globals (`console`,
  `System`); calls through them resolve to `builtin:<recv>`.

## Workflow for adding/fixing a language

1. Inspect real node kinds: `cargo run --example dump -- tests/fixtures/probe/probe.kt`
   (write any probe file you need).
2. Edit the query in `src/lang/<language>.rs`.
3. `cargo test --test registry_test` — query compiles.
4. `cargo test --test <language>_test` — fixtures extract correctly.

`tests/typescript_test.rs` + `tests/fixtures/typescript/` is the reference for
what a language test should assert: function forms (plain/method/nested/etc.),
`containing_type`, param counts, call attribution incl. receivers, imports with
bound names, toplevel call capture, and a control-flow feature.
