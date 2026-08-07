use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Validated against tests/fixtures/csharp/*.cs (see tests/csharp_test.rs).
// Grammar shapes verified via `cargo run --example dump`:
// - `method_declaration` / `constructor_declaration` / `local_function_statement`
//   all carry `name:` + `parameters: (parameter_list)` fields. Expression-bodied
//   members (`int F() => x;`) are the same node with an `arrow_expression_clause`
//   body, so no extra pattern is needed. Property accessors and lambdas are
//   deliberately not captured (noise).
// - `invocation_expression` has `function:` / `arguments:` fields. The callee is
//   an `identifier` (plain call), `generic_name` (`Cast<int>(x)`), a
//   `member_access_expression` (`obj.M(x)`), or a
//   `conditional_access_expression` (`obj?.M(x)`).
// - In `member_access_expression`, `this`/`base` are *anonymous* tokens under
//   the `expression:` field, so the receiver is captured with the bare `_`
//   wildcard (matches anonymous nodes) rather than `(_)` (named only). One
//   pattern with the wildcard also covers named receivers — a single pattern
//   avoids the dedupe-drops-recv hazard described in java.rs.
// - `object_creation_expression` type is an `identifier`, `generic_name`
//   (`new List<int>()`), or `qualified_name` (`new A.B.Widget()`); the simple
//   type name becomes `@call.name` so constructor calls link to constructors.
//   `arguments:` is optional (`new Widget { Size = 2 }` has only an
//   initializer). Implicit `new(...)` has no type name and is skipped.
// - `using_directive` puts the imported path in a `qualified_name` child (or a
//   bare `identifier` for `using System;`); the alias of `using F = ...;` is
//   the `name:` field. `!name` keeps the single-segment pattern from binding
//   the alias identifier as a path. `using static` / `global using` are the
//   same node with extra anonymous keywords, so the same patterns match.
// - Namespaces: `namespace_declaration` and `file_scoped_namespace_declaration`
//   both hold `name: (qualified_name | identifier)`.
// - Type captures (verified via probe dump): `field_declaration` wraps a
//   `variable_declaration` with a `type:` field (`identifier`, `generic_name`,
//   `qualified_name`, or `predefined_type` — primitives are skipped) and one or
//   more `variable_declarator name:` children. `property_declaration` carries
//   `type:`/`name:` fields directly (auto-properties and `{ get; init; }`
//   alike). Typed locals sit under `local_declaration_statement` — the SAME
//   `variable_declaration` node kind as fields, so both patterns are anchored
//   by their parent kind to keep fields and locals apart. `var` parses as
//   `(implicit_type)` with no identifier, so implicitly typed locals are only
//   recovered through the `variable_declarator` + `object_creation_expression`
//   initializer pattern (`var x = new Foo()`); `Foo y = new Foo()` matches
//   both the declared-type and constructed patterns (identical type either
//   way). `parameter` has `type:`/`name:` fields and covers method, ctor and
//   interface-signature params as `@local.*`. Primary constructors put a
//   `parameter_list` directly on `class_declaration`/`record_declaration`;
//   those params are additionally captured as `@field.*` (owner = the nearest
//   TYPE_KINDS ancestor, i.e. the declaring type). `base_list` children are
//   `identifier`, `qualified_name` (clean_type keeps the last segment), or
//   `generic_name` (inner identifier captured) on class/struct/interface/
//   record declarations alike. Ctor-body joins (`_store = store;`) use a bare
//   identifier on the left (no `this.`), indistinguishable from local
//   assignment — not captured; C# fields always have declared types, so the
//   declaration patterns suffice. Bare-field receivers (`_store.Save()`)
//   resolve through the field capture (resolver checks locals, then fields).
// - Call-site type args (probe-verified): `services.AddScoped<I, T>()` is an
//   `invocation_expression` whose `member_access_expression name:` is a
//   `generic_name (identifier, type_argument_list)`; the list's children are
//   bare `identifier`s for simple names, `qualified_name` for dotted ones and
//   `predefined_type` for primitives. Only bare identifiers are captured (the
//   quantified `@call.typearg` group stops at the first non-identifier), so
//   DI detection sees exactly-two-simple-name registrations and nothing else.
//   The typearg patterns overlap the plain generic ones on purpose — the
//   extractor's call dedupe merges them and keeps the typearg list.
const QUERY: &str = r#"
(method_declaration
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @deco.name
      (attribute_argument_list) @deco.args) @deco)
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @deco.name) @deco)
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(constructor_declaration
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(local_function_statement
  name: (identifier) @func.name
  parameters: (parameter_list) @func.params) @func.def

(invocation_expression
  function: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (generic_name (identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (member_access_expression
    expression: _ @call.recv
    name: (identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (member_access_expression
    expression: _ @call.recv
    name: (generic_name (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (member_access_expression
    expression: _ @call.recv
    name: (generic_name
      (identifier) @call.name
      (type_argument_list ((identifier) @call.typearg)+)))
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (generic_name
    (identifier) @call.name
    (type_argument_list ((identifier) @call.typearg)+))
  arguments: (argument_list) @call.args) @call

(invocation_expression
  function: (conditional_access_expression
    condition: (_) @call.recv
    (member_binding_expression
      name: (identifier) @call.name))
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (identifier) @call.name
  arguments: (argument_list)? @call.args) @call

(object_creation_expression
  type: (generic_name (identifier) @call.name)
  arguments: (argument_list)? @call.args) @call

(object_creation_expression
  type: (qualified_name
    name: (identifier) @call.name)
  arguments: (argument_list)? @call.args) @call

(using_directive
  (qualified_name) @import.path) @import

(using_directive
  !name
  (identifier) @import.path) @import

(using_directive
  name: (identifier) @import.name) @import

(namespace_declaration
  name: (qualified_name) @package.name)

(namespace_declaration
  name: (identifier) @package.name)

(file_scoped_namespace_declaration
  name: (qualified_name) @package.name)

(file_scoped_namespace_declaration
  name: (identifier) @package.name)

(field_declaration
  (variable_declaration
    type: [
      (identifier) @field.type
      (qualified_name) @field.type
      (generic_name (identifier) @field.type)
    ]
    (variable_declarator
      name: (identifier) @field.name)))

(property_declaration
  type: [
    (identifier) @field.type
    (qualified_name) @field.type
    (generic_name (identifier) @field.type)
  ]
  name: (identifier) @field.name)

(parameter
  type: [
    (identifier) @local.type
    (qualified_name) @local.type
    (generic_name (identifier) @local.type)
  ]
  name: (identifier) @local.name)

(local_declaration_statement
  (variable_declaration
    type: [
      (identifier) @local.type
      (qualified_name) @local.type
      (generic_name (identifier) @local.type)
    ]
    (variable_declarator
      name: (identifier) @local.name)))

(local_declaration_statement
  (variable_declaration
    (variable_declarator
      name: (identifier) @local.name
      (object_creation_expression
        type: [
          (identifier) @local.type
          (qualified_name) @local.type
          (generic_name (identifier) @local.type)
        ]))))

(class_declaration
  (parameter_list
    (parameter
      type: [
        (identifier) @field.type
        (qualified_name) @field.type
        (generic_name (identifier) @field.type)
      ]
      name: (identifier) @field.name)))

(record_declaration
  (parameter_list
    (parameter
      type: [
        (identifier) @field.type
        (qualified_name) @field.type
        (generic_name (identifier) @field.type)
      ]
      name: (identifier) @field.name)))

(class_declaration
  name: (identifier) @hier.type
  (base_list
    [
      (identifier) @hier.base
      (qualified_name) @hier.base
      (generic_name (identifier) @hier.base)
    ]))

(struct_declaration
  name: (identifier) @hier.type
  (base_list
    [
      (identifier) @hier.base
      (qualified_name) @hier.base
      (generic_name (identifier) @hier.base)
    ]))

(interface_declaration
  name: (identifier) @hier.type
  (base_list
    [
      (identifier) @hier.base
      (qualified_name) @hier.base
      (generic_name (identifier) @hier.base)
    ]))

(record_declaration
  name: (identifier) @hier.type
  (base_list
    [
      (identifier) @hier.base
      (qualified_name) @hier.base
      (generic_name (identifier) @hier.base)
    ]))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("struct_declaration", "name"),
    ("interface_declaration", "name"),
    ("record_declaration", "name"),
    ("enum_declaration", "name"),
];

const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_statement",
];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_statement",
    "switch_expression",
    "conditional_expression",
];

const BUILTIN_RECEIVERS: &[&str] = &["Console", "Math", "Convert"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::CSharp,
        tree_sitter_c_sharp::LANGUAGE.into(),
        &["cs"],
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

const STRING_KINDS: &[&str] = &[
    "string_literal",
    "verbatim_string_literal",
    "raw_string_literal",
    "interpolated_string_expression",
];
