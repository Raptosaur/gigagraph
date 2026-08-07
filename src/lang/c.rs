use super::LangSpec;
use crate::types::{ImportStyle, Lang};

/// Function definitions must cover pointer-returning declarators, which nest
/// `function_declarator` inside `pointer_declarator` (one level per `*`; three
/// levels covered here — `T f()`, `T *f()`, `T **f()`, `T ***f()`).
///
/// Calls come in three shapes: plain `f(x)` (also covers calls through a
/// function-pointer variable), member function pointers `s.fn(x)` /
/// `p->vtable->fn(x)` (a `field_expression` callee; the argument side becomes
/// the receiver), and explicit dereference `(*fp)(x)`.
const QUERY: &str = r#"
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

(preproc_include
  path: (string_literal) @import.path) @import

(preproc_include
  path: (system_lib_string) @import.path.system) @import
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "field_identifier", "type_identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("struct_specifier", "name"),
    ("union_specifier", "name"),
    ("enum_specifier", "name"),
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "do_statement"];

const BRANCH_KINDS: &[&str] = &["if_statement", "switch_statement", "conditional_expression"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::C,
        tree_sitter_c::LANGUAGE.into(),
        &["c", "h"],
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
