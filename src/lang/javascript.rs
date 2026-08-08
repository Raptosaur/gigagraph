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
///
/// `const handleClick = useCallback(() => {...}, [deps])` and the HOC forms
/// (`memo`, `forwardRef`, MobX `observer`) bind a real, searchable function to
/// a name, so the wrapper-call patterns credit the inner arrow/function to the
/// declared name — otherwise every React event handler in a codebase would be
/// invisible to the graph. The whitelist is deliberate and only covers
/// wrappers that RETURN the function they are given: a bare
/// `const x = xs.map(i => ...)` binds an array, not a function, so the general
/// "arrow anywhere in a call" shape would mislabel it. `useMemo` is excluded
/// for the same reason — it returns the arrow's RESULT, not the arrow.
///
/// Curried test declarations (`it.each([...])("adds %i", fn)`) call the RESULT
/// of a call, a shape the two plain call patterns cannot match: both need
/// `function:` to be an identifier or a member expression. The extra pattern
/// re-emits the inner `it.each` callee with the OUTER argument list, so the
/// case title reaches `arg_lits` — without it every `.each` table in a suite
/// is invisible to test discovery. Scoped by `#any-of?` to the test modifiers
/// so ordinary currying (`connect(a)(B)`) does not gain duplicate call rows;
/// the inner `it.each([...])` row survives too (different byte range, so the
/// extractor's dedup keeps both) carrying the table instead of the title.
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

(variable_declarator
  name: (identifier) @func.name
  value: (call_expression
    function: (identifier) @_wrap
    arguments: (arguments . [(arrow_function) (function_expression)] @func.body))
  (#any-of? @_wrap "useCallback" "memo" "forwardRef" "observer")) @func.def

(variable_declarator
  name: (identifier) @func.name
  value: (call_expression
    function: (member_expression property: (property_identifier) @_wrap)
    arguments: (arguments . [(arrow_function) (function_expression)] @func.body))
  (#any-of? @_wrap "useCallback" "memo" "forwardRef" "observer")) @func.def

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

(call_expression
  function: (call_expression
    function: (member_expression
      object: (_) @call.recv
      property: (property_identifier) @call.name))
  arguments: (arguments) @call.args
  (#any-of? @call.name "each" "only" "skip" "concurrent" "failing" "sequential" "todo")) @call

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
