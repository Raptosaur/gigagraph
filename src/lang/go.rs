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
