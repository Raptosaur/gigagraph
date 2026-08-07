use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-kotlin-ng. Notable grammar shapes (verified via
// `cargo run --example dump`):
// - `function_declaration` has a `name` field but its parameter list
//   (`function_value_parameters`) is an unnamed child.
// - `call_expression` has no fields; the callee is the first named child
//   (an `identifier` for plain/constructor-style calls, a
//   `navigation_expression` for receiver calls) followed by
//   `value_arguments` and/or a trailing `annotated_lambda`. BUT a call with
//   both parens and a trailing lambda (`route("/api") { ... }`) parses as
//   TWO nested call_expressions: `(call (call ident value_arguments)
//   annotated_lambda)` — the generic pattern captures only the INNER node,
//   whose byte range excludes the lambda body. Ktor prefix nesting needs the
//   OUTER range (endpoint detection joins `route(...)` prefixes onto verb
//   calls by byte containment), so one extra pattern captures the outer node
//   for calls named exactly `route`. That duplicates each `route(..){..}`
//   call in the stream (inner + outer span — the extractor dedupes by byte
//   range, and the ranges differ); the `#eq?` gate keeps the duplication
//   away from every other call, and `route` itself never becomes an endpoint
//   or resolves to project code, so the extra call site is harmless noise.
// - `navigation_expression` has exactly two named children: receiver and
//   member identifier.
// - `import` holds a `qualified_identifier` plus, for `import a.b.C as D`,
//   a trailing alias `identifier`. Wildcard imports keep only the package
//   part in the qualified_identifier (the `.*` is anonymous tokens).
// - Annotations with arguments (`@GET("/x")`, `@GetMapping("/x")`) sit in a
//   `modifiers` child INSIDE the `function_declaration` node, as
//   `(annotation (constructor_invocation (user_type ...) (value_arguments)))`.
//   Because the annotation lies within the function's byte range, the deco
//   pattern must capture `@func.def` in the same match (same-match
//   association) — nearest-following-function would skip to the NEXT
//   function. Marker annotations without arguments (`@Deprecated`) are not
//   captured (no argument payload to interpret).
const QUERY: &str = r#"
(function_declaration
  name: (identifier) @func.name
  (function_value_parameters) @func.params) @func.def

(call_expression
  . (identifier) @call.name
  (value_arguments) @call.args) @call

(call_expression
  . (identifier) @call.name
  (annotated_lambda) @call.args) @call

(call_expression
  . (navigation_expression
      . (_) @call.recv
      (identifier) @call.name .)
  (value_arguments) @call.args) @call

(call_expression
  . (navigation_expression
      . (_) @call.recv
      (identifier) @call.name .)
  (annotated_lambda) @call.args) @call

((call_expression
  . (call_expression
      . (identifier) @call.name
      (value_arguments) @call.args)
  (annotated_lambda)) @call
  (#eq? @call.name "route"))

(function_declaration
  (modifiers
    (annotation
      (constructor_invocation
        (user_type (identifier) @deco.name)
        (value_arguments) @deco.args)) @deco)
  name: (identifier) @func.name
  (function_value_parameters) @func.params) @func.def

(import (qualified_identifier) @import.path) @import

(import
  (qualified_identifier)
  (identifier) @import.name) @import

((import (qualified_identifier (identifier) @import.name .) .) @import
  (#not-match? @import "[*]"))

(package_header (qualified_identifier) @package.name)

(property_declaration
  (variable_declaration (identifier) @const.name)
  (string_literal) @const.value)

(function_body (string_literal) @ret.str)

(return_expression (string_literal) @ret.str)

((class_declaration
  (modifiers
    (annotation
      (constructor_invocation
        (user_type (identifier) @deco.name)
        (value_arguments) @deco.args)) @deco))
  (#eq? @deco.name "RequestMapping"))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("object_declaration", "name"),
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "do_while_statement"];

const BRANCH_KINDS: &[&str] = &["if_expression", "when_expression", "catch_block"];

const BUILTIN_RECEIVERS: &[&str] = &["System"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Kotlin,
        tree_sitter_kotlin_ng::LANGUAGE.into(),
        &["kt", "kts"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::DottedPackage,
        BUILTIN_RECEIVERS,
    )
}

const STRING_KINDS: &[&str] = &["string_literal"];
