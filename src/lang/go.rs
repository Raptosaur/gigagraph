use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Validated against tests/fixtures/go/*.go (see tests/go_test.rs).
//
// Notes:
// - `method_declaration` covers receiver methods (`func (s *Server) Run()`).
//   The receiver's type lives in the `receiver:` parameter list — a sibling
//   field, not an ancestor node — so `TYPE_KINDS` cannot express it and
//   methods get `containing_type: None`. Known limitation.
// - Func literals (`func() { ... }`) are not captured as `@func.def`; calls
//   inside goroutine/defer closures therefore attribute to the enclosing
//   named function, which is the behavior we want.
// - `go f()` / `defer f()` need no dedicated patterns: the inner
//   `call_expression` matches anywhere in the tree.
// - Imports capture each `import_spec` (not the whole declaration) so grouped
//   `import ( ... )` blocks yield one import per path. The path-only pattern
//   matches every spec (extractor strips the quotes); the aliased pattern
//   merges its `@import.name` onto the same node. Dot (`. "pkg"`) and blank
//   (`_ "pkg"`) imports only bind the path — their `name:` field is a `dot` /
//   `blank_identifier`, not a `package_identifier`.
// - `ImportStyle::PathLike` is a placeholder; Go-specific external-package
//   classification is handled separately.
//
// Type captures (verified via probe dump against tree-sitter-go):
// - Struct fields: `type_declaration > type_spec (name: type_identifier,
//   type: struct_type > field_declaration_list > field_declaration)`. The
//   owner (`type_spec` name) is a SIBLING of the struct body, not an ancestor
//   with a name field, so the pattern captures it explicitly as @field.owner
//   (overrides the extractor's ancestor scan — which would find nothing here,
//   TYPE_KINDS being empty). Field type shapes: bare `type_identifier`,
//   `pointer_type > type_identifier` (`*Logger`), `qualified_type` with a
//   `name:` field (`sql.DB` — NOT `field:`), and `pointer_type >
//   qualified_type` (`*sql.DB`). The inner `type_identifier` is captured
//   directly because `clean_type` rejects `*`. Embedded fields have no
//   `name:` field and are skipped.
// - Locals: `parameter_declaration` (same four type shapes) and
//   `var_declaration > var_spec`. The parameter pattern is deliberately
//   unanchored, so it also matches the receiver list of a
//   `method_declaration` — `func (s *Server) Handle()` yields local
//   `s -> Server` in Handle — and interface `method_elem` params (toplevel,
//   dropped by the extractor).
// - Receiver-resolution gap: `s.store.Save(1)` carries recv text `s.store`;
//   the resolver's receiver_binding has no receiver-name awareness (`s` is
//   not `this`/`self`) and its bare branch rejects dotted text, so struct
//   fields reached through a receiver do NOT hit the type rung today. The
//   @field captures still populate the tables for a future rung. What DOES
//   resolve now: bare typed params/vars (`store OrderStore` + `store.Save()`).
// - Constructed locals (`z := NewServer()`) are skipped: a query can only
//   capture the callee name ("NewServer"), which is a function, not a type,
//   and the `New` prefix cannot be stripped in a query. Known gap.
// - No @hier patterns: Go interface satisfaction is structural; there is no
//   implements/extends clause to capture.
const QUERY: &str = r#"
(function_declaration
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(method_declaration
  name: (field_identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(call_expression
  function: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (selector_expression
    operand: (_) @call.recv
    field: (field_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(import_spec
  path: (interpreted_string_literal) @import.path) @import

(import_spec
  name: (package_identifier) @import.name
  path: (interpreted_string_literal) @import.path) @import

(package_clause (package_identifier) @package.name)

(type_declaration
  (type_spec
    name: (type_identifier) @field.owner
    type: (struct_type
      (field_declaration_list
        (field_declaration
          name: (field_identifier) @field.name
          type: [
            (type_identifier) @field.type
            (pointer_type (type_identifier) @field.type)
            (qualified_type name: (type_identifier) @field.type)
            (pointer_type (qualified_type name: (type_identifier) @field.type))
          ])))))

(parameter_declaration
  name: (identifier) @local.name
  type: [
    (type_identifier) @local.type
    (pointer_type (type_identifier) @local.type)
    (qualified_type name: (type_identifier) @local.type)
    (pointer_type (qualified_type name: (type_identifier) @local.type))
  ])

(var_declaration
  (var_spec
    name: (identifier) @local.name
    type: [
      (type_identifier) @local.type
      (pointer_type (type_identifier) @local.type)
      (qualified_type name: (type_identifier) @local.type)
      (pointer_type (qualified_type name: (type_identifier) @local.type))
    ]))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "field_identifier", "type_identifier"];

// Methods can't resolve their receiver type via ancestry (see note above).
const TYPE_KINDS: &[(&str, &str)] = &[];

// `for` is Go's only loop (range-for is also a `for_statement`).
const LOOP_KINDS: &[&str] = &["for_statement"];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Go,
        tree_sitter_go::LANGUAGE.into(),
        &["go"],
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

const STRING_KINDS: &[&str] = &["interpreted_string_literal", "raw_string_literal"];
