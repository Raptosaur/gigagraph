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
// - DI type captures (verified via probe dumps):
//   - `class_parameter` is `modifiers? ('val'|'var')? identifier ':' type`;
//     the binding keyword is an anonymous token, so `["val" "var"]` in the
//     pattern restricts @field captures to real constructor properties —
//     plain (non-val) ctor params are NOT emitted as fields. They are not
//     emitted as locals either: Kotlin primary constructors produce no
//     function node, so a @local capture there would be dropped by the
//     extractor's innermost-function attribution anyway.
//   - `user_type` is `identifier ('.' identifier)*` with optional trailing
//     `type_arguments`; the `(identifier) @x .` anchor picks the last
//     segment of a qualified name (`com.example.Clock` -> `Clock`) and
//     deliberately fails on generics (`List<Order>` ends in type_arguments),
//     matching clean_type's wholesale rejection of generics. `T?` wraps the
//     `user_type` in a `nullable_type`; dedicated patterns unwrap it for
//     ctor properties and function params.
//   - `property_declaration` holds the type INSIDE `variable_declaration`
//     (`(variable_declaration (identifier) (user_type))`); a constructed
//     initializer (`val x = OrderStore()`) is a sibling `call_expression`
//     whose callee is its first named child. Class-body properties become
//     @field (the `class_body` wrapper keeps method-body locals from being
//     promoted to fields — containing_type would find the class ancestor
//     for those too); `enum_class_body` holds the identical
//     `property_declaration` shape (probe-verified), so each field pattern
//     has an enum-body twin, and nullable properties (`val t: Tracer? =
//     null`) unwrap `nullable_type` at the property position just like
//     params do. The unconstrained @local twins rely on the extractor
//     dropping bindings outside any function. Constructed captures are
//     gated on an uppercase callee (`#match? "^[A-Z]"`).
//   - Supertypes live in `(delegation_specifiers (delegation_specifier ...))`
//     on both `class_declaration` (also covers interfaces and enum classes)
//     and `object_declaration`; the payload is a `constructor_invocation`
//     for `Base()`, a bare `user_type` for interfaces, and an
//     `explicit_delegation` for `Iface by impl`.
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

(class_parameter
  ["val" "var"]
  (identifier) @field.name
  (user_type (identifier) @field.type .))

(class_parameter
  ["val" "var"]
  (identifier) @field.name
  (nullable_type (user_type (identifier) @field.type .)))

(parameter
  (identifier) @local.name
  (user_type (identifier) @local.type .))

(parameter
  (identifier) @local.name
  (nullable_type (user_type (identifier) @local.type .)))

(class_body
  (property_declaration
    (variable_declaration
      (identifier) @field.name
      (user_type (identifier) @field.type .))))

(class_body
  (property_declaration
    (variable_declaration
      (identifier) @field.name
      (nullable_type (user_type (identifier) @field.type .)))))

((class_body
  (property_declaration
    (variable_declaration (identifier) @field.name .)
    (call_expression
      . (identifier) @field.type
      (value_arguments))))
  (#match? @field.type "^[A-Z]"))

(enum_class_body
  (property_declaration
    (variable_declaration
      (identifier) @field.name
      (user_type (identifier) @field.type .))))

(enum_class_body
  (property_declaration
    (variable_declaration
      (identifier) @field.name
      (nullable_type (user_type (identifier) @field.type .)))))

((enum_class_body
  (property_declaration
    (variable_declaration (identifier) @field.name .)
    (call_expression
      . (identifier) @field.type
      (value_arguments))))
  (#match? @field.type "^[A-Z]"))

(property_declaration
  (variable_declaration
    (identifier) @local.name
    (user_type (identifier) @local.type .)))

((property_declaration
  (variable_declaration (identifier) @local.name .)
  (call_expression
    . (identifier) @local.type
    (value_arguments)))
  (#match? @local.type "^[A-Z]"))

(class_declaration
  name: (identifier) @hier.type
  (delegation_specifiers
    (delegation_specifier
      [(user_type (identifier) @hier.base .)
       (constructor_invocation (user_type (identifier) @hier.base .))
       (explicit_delegation (user_type (identifier) @hier.base .))])))

(object_declaration
  name: (identifier) @hier.type
  (delegation_specifiers
    (delegation_specifier
      [(user_type (identifier) @hier.base .)
       (constructor_invocation (user_type (identifier) @hier.base .))
       (explicit_delegation (user_type (identifier) @hier.base .))])))
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
