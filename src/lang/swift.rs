use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-swift 0.7 (alex-pinkus grammar).
//
// Grammar notes (verified via `cargo run --example dump`):
// - `struct` / `class` / `enum` / `actor` / `extension` are all
//   `class_declaration`, distinguished by the `declaration_kind` field; the
//   `name` field is a `type_identifier` (or `user_type` for extensions), so a
//   single TYPE_KINDS entry covers them all, including methods declared in
//   extensions.
// - Function parameters are direct fieldless `parameter` children of
//   `function_declaration` / `init_declaration`; there is NO parameter-list
//   wrapper node, so `@func.params` cannot be captured and param counts are
//   not available for Swift (they extract as 0).
// - `call_expression` wraps the callee expression (bare `simple_identifier`
//   or `navigation_expression`) plus a `call_suffix`. Subscript access
//   (`storage[key]`) is ALSO a `call_expression`; it is excluded by requiring
//   a literal `(` inside `value_arguments`. Trailing-closure-only calls
//   (`items.map { .. }`) have a `lambda_literal` as the first named child of
//   `call_suffix` and no `value_arguments`.
// - `Foo(...)` initializer calls are plain `call_expression`s with a
//   `simple_identifier` callee, so they come out as calls named `Foo`.
// - `init` declarations carry the anonymous `init` token in the `name` field.
// - In binary expressions the grammar hangs the `call_suffix` off the WHOLE
//   expression (`title + String(x)` is a `call_expression` whose callee child
//   is the `additive_expression` `title + String`); the real callee is that
//   expression's `rhs`, covered by the wildcard `(_ rhs: ...)` patterns.
// - Imports are `import_declaration` with a single `identifier` holding one
//   `simple_identifier` per dotted component; the last component is the name
//   bound in source.
// - DI type captures (verified via probe dump): `let x: T` is a
//   `property_declaration` EVERYWHERE — class bodies and function bodies use
//   the same node kind — with the field name `name:` doing double duty:
//   `name: (pattern bound_identifier: (simple_identifier))` for the binding
//   and `(type_annotation (user_type (type_identifier)))` for the type.
//   Disambiguation choice: the `@field` variant is restricted to direct
//   `class_body` children (covers class/struct/enum/actor/extension stored
//   properties, since all use `class_body`); the `@local` variant is left
//   unrestricted because the extractor attributes `@local` to the innermost
//   containing function and DROPS bindings outside any function, so the
//   class-body matches it also produces are discarded harmlessly.
// - `parameter` carries doubled `name:` fields at runtime — `name:
//   (simple_identifier)` for the bound name and `name: (user_type)` for the
//   type — but the grammar's STATIC node-types don't admit `user_type` under
//   the `name` field (same story for `type_annotation`'s `name:`), so
//   field-qualified patterns die with "Impossible pattern" at query-compile
//   time. The parameter pattern is therefore positional with a `.` adjacency
//   anchor, which also guarantees the identifier captured is the one directly
//   before the type: with an external argument label (`func send(to target:
//   Store)`, label under a separate `external_name:` field) the anchor binds
//   `target`, never `to`. Covers function, init, and protocol-requirement
//   parameters alike.
// - Constructed locals (`let x = DbStore()`) are `property_declaration` with
//   a `value: (call_expression (simple_identifier))` initializer; the
//   uppercase `#match?` guard keeps ordinary calls (`let n = count()`) out.
//   The `.` anchor between the pattern and the initializer excludes
//   `let y: Store = DbStore()` (a `type_annotation` sits between), which is
//   already covered — with the declared type winning — by the annotated
//   pattern, so no duplicate local is emitted.
// - Inheritance/conformance is one `inheritance_specifier` child per base
//   (`inherits_from: (user_type (type_identifier))`) on `class_declaration`
//   (class/struct alike) and `protocol_declaration` (protocol inheritance);
//   one @hier match fires per specifier. Extension conformance
//   (`extension Foo: Codable`) is NOT captured: the extension's `name` field
//   is a `user_type`, not a `type_identifier`, and it declares no type.
const QUERY: &str = r#"
(function_declaration
  name: (simple_identifier) @func.name) @func.def

(protocol_function_declaration
  name: (simple_identifier) @func.name) @func.def

(init_declaration
  name: "init" @func.name) @func.def

(call_expression
  (simple_identifier) @call.name
  (call_suffix (value_arguments "(") @call.args)) @call

(call_expression
  (simple_identifier) @call.name
  (call_suffix . (lambda_literal))) @call

(call_expression
  (navigation_expression
    target: (_) @call.recv
    suffix: (navigation_suffix
      suffix: (simple_identifier) @call.name))
  (call_suffix (value_arguments "(") @call.args)) @call

(call_expression
  (navigation_expression
    target: (_) @call.recv
    suffix: (navigation_suffix
      suffix: (simple_identifier) @call.name))
  (call_suffix . (lambda_literal))) @call

(call_expression
  (_ rhs: (simple_identifier) @call.name)
  (call_suffix (value_arguments "(") @call.args)) @call

(call_expression
  (_ rhs: (navigation_expression
    target: (_) @call.recv
    suffix: (navigation_suffix
      suffix: (simple_identifier) @call.name)))
  (call_suffix (value_arguments "(") @call.args)) @call

(import_declaration
  (identifier) @import.path) @import

(import_declaration
  (identifier (simple_identifier) @import.name .)) @import

(function_declaration
  (modifiers
    (attribute
      (user_type (type_identifier) @deco.name)) @deco)
  name: (simple_identifier) @func.name) @func.def

(control_transfer_statement (line_string_literal) @ret.str)

(class_body
  (property_declaration
    name: (pattern bound_identifier: (simple_identifier) @field.name)
    (type_annotation (user_type (type_identifier) @field.type))))

(property_declaration
  name: (pattern bound_identifier: (simple_identifier) @local.name)
  (type_annotation (user_type (type_identifier) @local.type)))

(parameter
  (simple_identifier) @local.name
  .
  (user_type (type_identifier) @local.type))

(property_declaration
  name: (pattern bound_identifier: (simple_identifier) @local.name)
  .
  value: (call_expression (simple_identifier) @local.type)
  (#match? @local.type "^[A-Z]"))

(class_declaration
  name: (type_identifier) @hier.type
  (inheritance_specifier
    inherits_from: (user_type (type_identifier) @hier.base)))

(protocol_declaration
  name: (type_identifier) @hier.type
  (inheritance_specifier
    inherits_from: (user_type (type_identifier) @hier.base)))
"#;

const IDENTIFIER_KINDS: &[&str] = &["simple_identifier", "type_identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("protocol_declaration", "name"),
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "repeat_while_statement"];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_statement",
    "guard_statement",
    "ternary_expression",
    "catch_block",
];

const BUILTIN_RECEIVERS: &[&str] = &["Swift"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Swift,
        tree_sitter_swift::LANGUAGE.into(),
        &["swift"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::Module,
        BUILTIN_RECEIVERS,
    )
}

const STRING_KINDS: &[&str] = &["line_string_literal"];
