use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-objc 3.0.2 (tree-sitter-grammars/amaanq, a
// tree-sitter-c derivative). Grammar notes (verified via
// `cargo run --example dump`):
//
// - Methods are `method_definition` nodes with NO fields: children are an
//   optional `method_type`, then the bare `identifier` selector segments
//   interleaved with `method_parameter` nodes, then the `compound_statement`.
//   The FIRST selector segment is captured as `@func.name` (`logAll` for
//   `- (void)logAll:items prefix:p`) — there is no single node holding the
//   full `logAll:prefix:` selector. There is no parameter-list wrapper node,
//   so `@func.params` cannot be captured and ObjC methods extract with
//   param_count 0 (same limitation as Swift).
// - `class_implementation` / `class_interface` expose NO `name` field (the
//   class name is a bare `identifier` child; only `category` / `superclass`
//   are fields), so TYPE_KINDS cannot resolve `containing_type` — ObjC
//   methods extract with containing_type None. Not fixable at query level.
// - Message sends are `message_expression` with a `receiver:` field and one
//   `method:` identifier PER selector segment. Anchoring `receiver . method`
//   captures only the first segment (`[self logAll:x prefix:y]` → one call
//   `logAll`, receiver `self`), instead of one phantom call per segment.
//   Arguments are loose expression children (no argument_list), so message
//   sends carry arg_count 0.
// - C constructs in `.m`/`.mm` (functions, plain calls, includes) parse
//   exactly as in tree-sitter-c; those patterns are copied from c.rs.
// - `#import`/`#include` are both `preproc_include`; `@import UIKit;` is a
//   `module_import` with `path: (identifier)`.
// - ObjC string literals `@"..."` are ordinary `string_literal` nodes (the
//   extractor's strip_quotes already drops the `@` prefix).
//
// React Native RCT_* macros (the reason this language exists in the index)
// are unparseable as ObjC, and the grammar ERROR-recovers them in three
// stable shapes, each captured below:
//
// 1. `RCT_EXPORT_METHOD(sel:(T *)arg ...)` → a `macro_type_specifier` with
//    `name: (identifier)` = the macro and an ERROR child holding a
//    `type_descriptor` whose text is the selector's first segment. Captured
//    as a decoration named `RCT_EXPORT_METHOD` with the selector in
//    `@deco.args` (harvested as an Ident ArgLit). The method BODY becomes
//    garbage nodes (often an uncaptured bogus `function_definition`), so
//    calls inside typed RCT method bodies attribute to `(toplevel)`.
// 2. `RCT_EXPORT_METHOD(reset)` (selector with no parameters) → a bogus but
//    clean `function_definition` with `type: RCT_EXPORT_METHOD` and
//    `declarator: (parenthesized_declarator (identifier))`. Captured as a
//    real function named after the selector PLUS a same-match decoration
//    named `RCT_EXPORT_METHOD`; body calls attribute correctly.
// 3. `RCT_EXPORT_MODULE();` → a bare `identifier` inside an ERROR node;
//    `RCT_EXPORT_MODULE(JsName);` → a clean `declaration` with
//    `declarator: (parenthesized_declarator)`. Both captured as decorations
//    (the latter with the JS name in `@deco.args`).
//
// The `#match? "^RCT_"` guards deviate from the "framework-agnostic query"
// rule deliberately: `macro_type_specifier` is also the legitimate parse of
// `NS_ENUM(...)`-style typedefs, and unguarded patterns would spray
// decorations over every macro ERROR-recovery in the file.
const QUERY: &str = r#"
(method_definition
  (method_type)
  . (identifier) @func.name) @func.def

(method_definition
  . (identifier) @func.name) @func.def

(function_definition
  declarator: (function_declarator
    declarator: (identifier) @func.name
    parameters: (parameter_list) @func.params)) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: (identifier) @func.name
      parameters: (parameter_list) @func.params))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (function_declarator
        declarator: (identifier) @func.name
        parameters: (parameter_list) @func.params)))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (pointer_declarator
        declarator: (function_declarator
          declarator: (identifier) @func.name
          parameters: (parameter_list) @func.params))))) @func.def

((function_definition
  type: (type_identifier) @deco @deco.name
  declarator: (parenthesized_declarator
    (identifier) @func.name)
  body: (compound_statement)) @func.def
  (#match? @deco.name "^RCT_"))

((macro_type_specifier
  name: (identifier) @deco.name
  (ERROR
    (type_descriptor) @deco.args)) @deco
  (#match? @deco.name "^RCT_"))

((macro_type_specifier
  name: (identifier) @deco.name
  . type: (type_descriptor) @deco.args) @deco
  (#match? @deco.name "^RCT_"))

((ERROR
  (identifier) @deco @deco.name)
  (#eq? @deco.name "RCT_EXPORT_MODULE"))

((declaration
  type: (type_identifier) @deco.name
  declarator: (parenthesized_declarator) @deco.args) @deco
  (#match? @deco.name "^RCT_"))

(call_expression
  function: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (field_expression
    argument: (_) @call.recv
    field: (field_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (parenthesized_expression
    (pointer_expression
      argument: (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(message_expression
  receiver: (_) @call.recv
  . method: (identifier) @call.name) @call

(preproc_include
  path: (string_literal) @import.path) @import

(preproc_include
  path: (system_lib_string) @import.path.system) @import

(module_import
  path: (identifier) @import.path) @import
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "field_identifier", "type_identifier"];

/// `class_implementation` / `class_interface` have no name field (see module
/// comment) — no ancestor kind can yield a containing type via field lookup.
// Empty name-field = positional fallback in the extractor (first
// identifier child) — the grammar has no `name` field on these nodes.
const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_implementation", ""),
    ("class_interface", ""),
    ("category_implementation", ""),
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "do_statement"];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_statement",
    "conditional_expression",
    "catch_clause",
];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::ObjC,
        tree_sitter_objc::LANGUAGE.into(),
        &["m", "mm"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        &[],
    )
}

const STRING_KINDS: &[&str] = &["string_literal"];
