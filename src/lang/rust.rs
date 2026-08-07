use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-rust 0.24. Notable grammar shapes (verified via
// `cargo run --example dump`):
// - `function_item` covers free fns, impl/trait methods, trait default
//   methods, nested fns; `async`/`const`/`unsafe` land in an anonymous
//   `function_modifiers` child, so one pattern covers them all. Trait method
//   signatures without a body are `function_signature_item` (not captured —
//   no body means no calls to attribute).
// - `parameters` counts `self_parameter` as a named child, so methods taking
//   `&self` report it in their param count.
// - `call_expression` callees: `identifier` (plain), `field_expression`
//   (method call — `value` is the receiver), `scoped_identifier` (path calls
//   like `Type::new` / `mod::f` — `path` is the receiver), and
//   `generic_function` wrapping any of the three (turbofish `f::<T>()`).
// - Macro invocations (`println!`, `format!`) are `macro_invocation` with a
//   `macro` field holding an `identifier` or `scoped_identifier`; arguments
//   are an opaque `token_tree`, so no `@call.args` for macros.
// - `attribute_item` (`#[get("/x")]`) is a SIBLING preceding the
//   `function_item`, so the extractor's nearest-following-function
//   association attaches it to the right fn (verified: a same-match sibling
//   pattern is unnecessary). Only HTTP-verb-ish attribute names are captured
//   (`#match?` filter) to keep `#[derive(...)]`/`#[cfg(...)]` noise out of
//   the decoration stream. Scoped forms (`#[actix_web::get(...)]`) are
//   captured by a second pattern binding the `scoped_identifier`'s leaf
//   `name` as `@deco.name`, so detection sees the same bare verb.
// - `use_declaration` argument shapes: bare `identifier`, `scoped_identifier`
//   (leaf `name` is the bound name), `use_as_clause` (`path` + `alias`),
//   `use_wildcard` (wraps the base path node), and `scoped_use_list`
//   (`path` base + `use_list` of leaves: identifiers, scoped identifiers,
//   as-clauses, wildcards).
const QUERY: &str = r#"
(function_item
  name: (identifier) @func.name
  parameters: (parameters) @func.params) @func.def

(call_expression
  function: (identifier) @call.name
  arguments: (arguments) @call.args) @call

(call_expression
  function: (field_expression
    value: (_) @call.recv
    field: (field_identifier) @call.name)
  arguments: (arguments) @call.args) @call

(call_expression
  function: (scoped_identifier
    path: (_) @call.recv
    name: (identifier) @call.name)
  arguments: (arguments) @call.args) @call

(call_expression
  function: (generic_function
    function: (identifier) @call.name)
  arguments: (arguments) @call.args) @call

(call_expression
  function: (generic_function
    function: (field_expression
      value: (_) @call.recv
      field: (field_identifier) @call.name))
  arguments: (arguments) @call.args) @call

(call_expression
  function: (generic_function
    function: (scoped_identifier
      path: (_) @call.recv
      name: (identifier) @call.name))
  arguments: (arguments) @call.args) @call

(macro_invocation
  macro: (identifier) @call.name) @call

(macro_invocation
  macro: (scoped_identifier
    path: (_) @call.recv
    name: (identifier) @call.name)) @call

((attribute_item
  (attribute
    (identifier) @deco.name
    arguments: (token_tree) @deco.args)) @deco
  (#match? @deco.name "^(get|post|put|delete|patch|head|options|route|connect|trace)$"))

((attribute_item
  (attribute
    (scoped_identifier
      name: (identifier) @deco.name)
    arguments: (token_tree) @deco.args)) @deco
  (#match? @deco.name "^(get|post|put|delete|patch|head|options|route|connect|trace)$"))

((attribute_item
  (attribute
    (identifier) @deco.name .)) @deco
  (#match? @deco.name "^(test|bench|rstest)$"))

((attribute_item
  (attribute
    (scoped_identifier
      name: (identifier) @deco.name) .)) @deco
  (#match? @deco.name "^(test|bench|rstest)$"))

(use_declaration
  argument: (identifier) @import.path @import.name) @import

(use_declaration
  argument: (scoped_identifier) @import.path) @import

(use_declaration
  argument: (scoped_identifier
    name: (identifier) @import.name)) @import

(use_declaration
  argument: (use_as_clause
    path: (_) @import.path
    alias: (identifier) @import.name)) @import

(use_declaration
  argument: (use_wildcard (_) @import.path)) @import

(use_declaration
  argument: (scoped_use_list
    path: (_) @import.path)) @import

(use_declaration
  argument: (scoped_use_list
    list: (use_list (identifier) @import.name))) @import

(use_declaration
  argument: (scoped_use_list
    list: (use_list
      (scoped_identifier
        name: (identifier) @import.name)))) @import

(use_declaration
  argument: (scoped_use_list
    list: (use_list
      (use_as_clause
        alias: (identifier) @import.name)))) @import
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "field_identifier", "type_identifier"];

/// Nearest ancestor wins: a method in an `impl` block reports the impl'd type
/// even when the impl sits inside a `mod`.
const TYPE_KINDS: &[(&str, &str)] = &[
    ("impl_item", "type"),
    ("struct_item", "name"),
    ("enum_item", "name"),
    ("union_item", "name"),
    ("trait_item", "name"),
    ("mod_item", "name"),
];

const LOOP_KINDS: &[&str] = &["for_expression", "while_expression", "loop_expression"];

const BRANCH_KINDS: &[&str] = &["if_expression", "match_expression"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Rust,
        tree_sitter_rust::LANGUAGE.into(),
        &["rs"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::DottedPackage,
        &[],
    )
}

const STRING_KINDS: &[&str] = &["string_literal", "raw_string_literal"];
