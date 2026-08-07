use super::LangSpec;
use crate::types::{ImportStyle, Lang};

/// Query fragments shared by the JavaScript and TypeScript grammars
/// (identical node kinds in both).
pub(crate) const CORE_QUERY: &str = r#"
(function_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(generator_function_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_definition
  name: (_) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(variable_declarator
  name: (identifier) @func.name
  value: [(arrow_function) (function_expression) (generator_function)] @func.body) @func.def

(pair
  key: (property_identifier) @func.name
  value: [(arrow_function) (function_expression)] @func.body) @func.def

(assignment_expression
  left: (member_expression
    property: (property_identifier) @func.name)
  right: [(arrow_function) (function_expression)] @func.body) @func.def

(call_expression
  function: (identifier) @call.name
  arguments: (arguments) @call.args) @call

(call_expression
  function: (member_expression
    object: (_) @call.recv
    property: (property_identifier) @call.name)
  arguments: (arguments) @call.args) @call

(new_expression
  constructor: (identifier) @call.name) @call

(import_statement
  source: (string (string_fragment) @import.path)) @import

(import_statement
  (import_clause (identifier) @import.name)) @import

(import_statement
  (import_clause (named_imports
    (import_specifier name: (identifier) @import.name)))) @import

(import_statement
  (import_clause (named_imports
    (import_specifier alias: (identifier) @import.name)))) @import

(import_statement
  (import_clause (namespace_import (identifier) @import.name))) @import

(export_statement
  source: (string (string_fragment) @import.path)) @import

(variable_declarator
  name: (identifier) @import.name
  value: (call_expression
    function: (identifier) @_req
    arguments: (arguments (string (string_fragment) @import.path)))
  (#eq? @_req "require")) @import

(variable_declarator
  name: (object_pattern (shorthand_property_identifier_pattern) @import.name)
  value: (call_expression
    function: (identifier) @_req
    arguments: (arguments (string (string_fragment) @import.path)))
  (#eq? @_req "require")) @import

(lexical_declaration
  (variable_declarator
    name: (identifier) @const.name
    value: (string) @const.value))
"#;

/// Class-property arrow methods: JS grammar calls the node `field_definition`.
const JS_EXTRA: &str = r#"
(field_definition
  property: (property_identifier) @func.name
  value: [(arrow_function) (function_expression)] @func.body) @func.def
"#;

pub(crate) const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "property_identifier",
    "shorthand_property_identifier",
    "private_property_identifier",
];

pub(crate) const TYPE_KINDS: &[(&str, &str)] = &[("class_declaration", "name"), ("class", "name")];

pub(crate) const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
];

pub(crate) const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_statement",
    "ternary_expression",
    "catch_clause",
];

pub(crate) const BUILTIN_RECEIVERS: &[&str] = &[
    "console",
    "Math",
    "JSON",
    "Object",
    "Array",
    "Promise",
    "Number",
    "String",
    "Date",
    "window",
    "document",
    "process",
    "globalThis",
    "Reflect",
    "Symbol",
    "Boolean",
    "RegExp",
    "Error",
    "Map",
    "Set",
    "Buffer",
    "performance",
    "crypto",
    "navigator",
    "localStorage",
    "sessionStorage",
];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::JavaScript,
        tree_sitter_javascript::LANGUAGE.into(),
        &["js", "mjs", "cjs", "jsx"],
        &format!("{CORE_QUERY}{JS_EXTRA}"),
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        BUILTIN_RECEIVERS,
    )
}

pub const STRING_KINDS: &[&str] = &["string", "template_string"];
