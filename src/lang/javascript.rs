use super::LangSpec;
use crate::types::{ImportStyle, Lang};

/// Query fragments shared by the JavaScript and TypeScript grammars
/// (identical node kinds in both).
///
/// `new_expression` comes in three overlapping patterns: name-only (covers
/// paren-less `new Thing;`), identifier constructor + arguments, and
/// member-form `new ns.Ctor(...)` with receiver + arguments. The extractor
/// dedups by (call range, name start), keeping the richest capture set, so
/// the overlap merges instead of duplicating. Argument capture matters:
/// CDK constructions (`new lambda.Function(this, 'X', { handler, code })`,
/// `new NodejsFunction({ entry })`, `new appsync.Resolver({ typeName })`)
/// carry their routing/handler props in the arguments object, which
/// `src/endpoints.rs` reads via the Ident-key/Str-value window shape.
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

(new_expression
  constructor: (identifier) @call.name
  arguments: (arguments) @call.args) @call

(new_expression
  constructor: (member_expression
    object: (_) @call.recv
    property: (property_identifier) @call.name)
  arguments: (arguments) @call.args) @call

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

(variable_declarator
  name: (identifier) @import.name
  value: (call_expression
    function: (call_expression
      function: (identifier) @_req
      arguments: (arguments (string (string_fragment) @import.path)))
    (#eq? @_req "require"))) @import

(lexical_declaration
  (variable_declarator
    name: (identifier) @const.name
    value: (string) @const.value))
"#;

/// Class-property arrow methods: JS grammar calls the node `field_definition`.
///
/// JS type captures (verified via probe dump) — untyped grammar, so the only
/// type evidence is construction: `const x = new T()` locals and
/// `this.x = new T()` fields (owner = nearest TYPE_KINDS ancestor of the
/// property name, i.e. the enclosing class). Heritage differs from TS:
/// `class_heritage` has no `extends_clause` wrapper — the base sits directly
/// under it as an `identifier` (or `member_expression` for `ns.Base`), and
/// the class name is a plain `identifier`, not `type_identifier`, so the
/// heritage patterns cannot be shared with TS. The two `new T()` shapes DO
/// parse identically in both grammars but live per-side anyway (TS_EXTRA
/// carries its own copies) to keep CORE_QUERY purely structural/function
/// captures.
const JS_EXTRA: &str = r#"
(field_definition
  property: (property_identifier) @func.name
  value: [(arrow_function) (function_expression)] @func.body) @func.def

(variable_declarator
  name: (identifier) @local.name
  value: (new_expression constructor: (identifier) @local.type))

(assignment_expression
  left: (member_expression
    object: (this)
    property: (property_identifier) @field.name)
  right: (new_expression constructor: (identifier) @field.type))

(class_declaration
  name: (identifier) @hier.type
  (class_heritage (identifier) @hier.base))

(class_declaration
  name: (identifier) @hier.type
  (class_heritage (member_expression property: (property_identifier) @hier.base)))
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
