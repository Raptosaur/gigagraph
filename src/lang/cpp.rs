use super::LangSpec;
use crate::types::{ImportStyle, Lang};

/// C++ extends the C patterns along two axes.
///
/// Declarator wrappers: besides C's `pointer_declarator` nesting (one level
/// per `*`, three levels covered), C++ adds `reference_declarator` for
/// reference-returning functions (`T &f()`).
///
/// Name shapes inside `function_declarator`:
/// - `identifier` — free functions and in-class constructors;
/// - `field_identifier` — in-class method definitions;
/// - `destructor_name` — destructors (`~Foo`, captured with the tilde);
/// - `qualified_identifier` — out-of-class definitions (`void Foo::bar()`,
///   `void ns::Foo::bar()`); the rightmost identifier becomes `@func.name`.
///   The class named in the qualifier is not an AST ancestor, so
///   `containing_type` resolves to the nearest enclosing namespace (or
///   nothing at global scope) — never to the class itself.
///
/// Operator overloads (`operator_name`) and lambdas are deliberately not
/// captured. Calls add qualified callees (`ns::f(x)`, `Klass::m(x)` — scope
/// becomes the receiver; for doubly-qualified `ns::Klass::m(x)` the inner
/// scope, i.e. the class, is the receiver), explicit template calls
/// (`f<T>(x)`), and `new Foo(...)` treated as a constructor call. Includes
/// are exactly C's.
const QUERY: &str = r#"
(function_definition
  declarator: (function_declarator
    declarator: [(identifier) (field_identifier) (destructor_name)] @func.name
    parameters: (parameter_list) @func.params)) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: [(identifier) (field_identifier) (destructor_name)] @func.name
      parameters: (parameter_list) @func.params))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (function_declarator
        declarator: [(identifier) (field_identifier) (destructor_name)] @func.name
        parameters: (parameter_list) @func.params)))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (pointer_declarator
        declarator: (function_declarator
          declarator: [(identifier) (field_identifier) (destructor_name)] @func.name
          parameters: (parameter_list) @func.params))))) @func.def

(function_definition
  declarator: (reference_declarator
    (function_declarator
      declarator: [(identifier) (field_identifier) (destructor_name)] @func.name
      parameters: (parameter_list) @func.params))) @func.def

(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier
      name: [(identifier) (destructor_name)] @func.name)
    parameters: (parameter_list) @func.params)) @func.def

(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier
      name: (qualified_identifier
        name: [(identifier) (destructor_name)] @func.name))
    parameters: (parameter_list) @func.params)) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: (qualified_identifier
        name: [(identifier) (destructor_name)] @func.name)
      parameters: (parameter_list) @func.params))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (function_declarator
        declarator: (qualified_identifier
          name: [(identifier) (destructor_name)] @func.name)
        parameters: (parameter_list) @func.params)))) @func.def

(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: (qualified_identifier
        name: (qualified_identifier
          name: [(identifier) (destructor_name)] @func.name))
      parameters: (parameter_list) @func.params))) @func.def

(function_definition
  declarator: (reference_declarator
    (function_declarator
      declarator: (qualified_identifier
        name: [(identifier) (destructor_name)] @func.name)
      parameters: (parameter_list) @func.params))) @func.def

(call_expression
  function: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (field_expression
    argument: (_) @call.recv
    field: (field_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (qualified_identifier
    scope: (_) @call.recv
    name: (identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (qualified_identifier
    name: (qualified_identifier
      scope: (_) @call.recv
      name: (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (template_function
    name: (identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (qualified_identifier
    scope: (_) @call.recv
    name: (template_function
      name: (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(call_expression
  function: (parenthesized_expression
    (pointer_expression
      argument: (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(new_expression
  type: (type_identifier) @call.name
  arguments: (argument_list) @call.args) @call

(new_expression
  type: (qualified_identifier
    scope: (_) @call.recv
    name: (type_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(preproc_include
  path: (string_literal) @import.path) @import

(preproc_include
  path: (system_lib_string) @import.path.system) @import
"#;

const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "field_identifier",
    "type_identifier",
    "namespace_identifier",
];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_specifier", "name"),
    ("struct_specifier", "name"),
    ("union_specifier", "name"),
    ("enum_specifier", "name"),
    ("namespace_definition", "name"),
];

const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "for_range_loop",
    "while_statement",
    "do_statement",
];

const BRANCH_KINDS: &[&str] = &["if_statement", "switch_statement", "conditional_expression"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Cpp,
        tree_sitter_cpp::LANGUAGE.into(),
        &["cpp", "cc", "cxx", "hpp", "hh"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        &["std"],
    )
}

const STRING_KINDS: &[&str] = &["string_literal", "raw_string_literal"];
