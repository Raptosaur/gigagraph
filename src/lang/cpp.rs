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
///
/// DI type captures (shapes verified via `cargo run --example dump`):
/// - Fields: `Store plain_;` has `declarator: (field_identifier)` directly;
///   `Store* ptr_;` wraps it in `pointer_declarator` (field-named child) and
///   `Store& ref_;` in `reference_declarator` (positional child, no field
///   name). Smart-pointer members — the C++ DI idiom — capture the wrapped
///   type: `std::unique_ptr<Store>` is `qualified_identifier > template_type >
///   template_argument_list > type_descriptor`, bare `unique_ptr<Store>`
///   (via `using`) starts at `template_type`; both are gated on
///   unique_ptr/shared_ptr/weak_ptr so `vector<T>` members don't fake-narrow.
/// - Params/locals: `parameter_declaration` mirrors the three field declarator
///   shapes (plus the qualified smart-pointer form); `Store s;` and
///   `Store t = ...;` are `declaration` with a plain/`init_declarator`
///   declarator; `auto u = Store();` captures the callee identifier as the
///   type, filtered to uppercase-initial so `auto v = make_store();` is not
///   mistaken for a type. `auto p = std::make_unique<Store>()` is NOT
///   captured (would need template-argument plumbing through the call).
/// - Hierarchy: `class D : public B, public I` — `base_class_clause` holds
///   `access_specifier` and `type_identifier` named children interleaved; the
///   single-child pattern matches once per base. Same for `struct_specifier`.
///
/// KNOWN LIMIT: field-based narrowing only helps methods *defined in-class* —
/// out-of-class definitions (`void Foo::bar()`) get the enclosing namespace,
/// not the class, as `containing_type` (see above), so their self-qualified
/// field lookups miss the owner. Also `this->field_` receivers are dropped by
/// the resolver (bare-branch rejects `-`); plain `field_->m()` works.
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

(field_declaration
  type: (type_identifier) @field.type
  declarator: (field_identifier) @field.name)

(field_declaration
  type: (type_identifier) @field.type
  declarator: (pointer_declarator
    declarator: (field_identifier) @field.name))

(field_declaration
  type: (type_identifier) @field.type
  declarator: (reference_declarator
    (field_identifier) @field.name))

((field_declaration
   type: (qualified_identifier
     name: (template_type
       name: (type_identifier) @_smart
       arguments: (template_argument_list
         (type_descriptor type: (type_identifier) @field.type))))
   declarator: (field_identifier) @field.name)
  (#match? @_smart "^(unique_ptr|shared_ptr|weak_ptr)$"))

((field_declaration
   type: (template_type
     name: (type_identifier) @_smart
     arguments: (template_argument_list
       (type_descriptor type: (type_identifier) @field.type)))
   declarator: (field_identifier) @field.name)
  (#match? @_smart "^(unique_ptr|shared_ptr|weak_ptr)$"))

(parameter_declaration
  type: (type_identifier) @local.type
  declarator: (identifier) @local.name)

(parameter_declaration
  type: (type_identifier) @local.type
  declarator: (pointer_declarator
    declarator: (identifier) @local.name))

(parameter_declaration
  type: (type_identifier) @local.type
  declarator: (reference_declarator
    (identifier) @local.name))

((parameter_declaration
   type: (qualified_identifier
     name: (template_type
       name: (type_identifier) @_smart
       arguments: (template_argument_list
         (type_descriptor type: (type_identifier) @local.type))))
   declarator: (identifier) @local.name)
  (#match? @_smart "^(unique_ptr|shared_ptr|weak_ptr)$"))

(declaration
  type: (type_identifier) @local.type
  declarator: (identifier) @local.name)

(declaration
  type: (type_identifier) @local.type
  declarator: (init_declarator
    declarator: (identifier) @local.name))

((declaration
   type: (placeholder_type_specifier)
   declarator: (init_declarator
     declarator: (identifier) @local.name
     value: (call_expression
       function: (identifier) @local.type)))
  (#match? @local.type "^[A-Z]"))

(class_specifier
  name: (type_identifier) @hier.type
  (base_class_clause (type_identifier) @hier.base))

(struct_specifier
  name: (type_identifier) @hier.type
  (base_class_clause (type_identifier) @hier.base))
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
