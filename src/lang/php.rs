use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Validated against tests/fixtures/php/*.php (see tests/php_test.rs).
//
// Notes:
// - `method_declaration` covers instance methods, static methods, constructors
//   (`__construct`) and interface/abstract signatures alike; the nearest
//   TYPE_KINDS ancestor (class/interface/trait/enum) supplies `containing_type`.
// - Closures/arrow functions are only captured when assigned to a variable
//   (`$f = function () {}` / `$f = fn() => ...`); the variable's inner `name`
//   node (no `$`) becomes the function name and the extractor probes
//   `@func.body` for the `parameters` field.
// - Receivers are captured faithfully: `$this->m()` yields recv text `$this`
//   (note: the resolver's this/self check compares against literal
//   "this"/"self", so `$this` will NOT match — handled outside this spec),
//   `self::m()` / `static::m()` yield a `relative_scope` node whose text
//   ("self"/"static") is captured as-is.
// - `new Foo(...)` / `new \A\B\Foo(...)` capture the class simple name as
//   `@call.name` (last segment of a `qualified_name`). `new $class(...)` and
//   anonymous classes are skipped (no clean name).
// - `use A\B\C;` binds its last segment as `@import.name`; the trailing anchor
//   keeps aliased declarations out of that pattern so `use X as Y;` binds only
//   `Y`. Grouped `use A\{B, C};` captures the shared `namespace_name` prefix as
//   the path and each group member as a bound name (merged by `@import` node
//   identity). `use function`/`use const` parse identically to plain `use` and
//   are captured the same way.
// - `require`/`require_once`/`include`/`include_once` are captured only with a
//   plain string argument; concatenations (`__DIR__ . '/x.php'`) are skipped
//   (no faithful literal path).
// - Import paths keep raw backslash separators (`App\Services\Mailer`);
//   ImportStyle stays DottedPackage per stub — PHP-specific classification is
//   handled separately.
const QUERY: &str = r#"
(function_definition
  name: (name) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  name: (name) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  attributes: (attribute_list
    (attribute_group
      (attribute
        (name) @deco.name
        parameters: (arguments) @deco.args) @deco))
  name: (name) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  attributes: (attribute_list
    (attribute_group
      (attribute (name) @deco.name) @deco))
  name: (name) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(assignment_expression
  left: (variable_name (name) @func.name)
  right: [(anonymous_function) (arrow_function)] @func.body) @func.def

(function_call_expression
  function: (name) @call.name
  arguments: (arguments) @call.args) @call

(function_call_expression
  function: (qualified_name (name) @call.name .)
  arguments: (arguments) @call.args) @call

(member_call_expression
  object: (_) @call.recv
  name: (name) @call.name
  arguments: (arguments) @call.args) @call

(nullsafe_member_call_expression
  object: (_) @call.recv
  name: (name) @call.name
  arguments: (arguments) @call.args) @call

(scoped_call_expression
  scope: (_) @call.recv
  name: (name) @call.name
  arguments: (arguments) @call.args) @call

(object_creation_expression
  (name) @call.name
  (arguments) @call.args) @call

(object_creation_expression
  (qualified_name (name) @call.name .)
  (arguments) @call.args) @call

(namespace_use_declaration
  (namespace_use_clause
    (qualified_name (name) @import.name .) @import.path .)) @import

(namespace_use_declaration
  (namespace_use_clause
    (qualified_name) @import.path
    alias: (name) @import.name)) @import

(namespace_use_declaration
  (namespace_use_clause . (name) @import.path @import.name .)) @import

(namespace_use_declaration
  (namespace_name) @import.path
  body: (namespace_use_group
    (namespace_use_clause (name) @import.name))) @import

(require_expression (string (string_content) @import.path)) @import

(require_once_expression (string (string_content) @import.path)) @import

(include_expression (string (string_content) @import.path)) @import

(include_once_expression (string (string_content) @import.path)) @import

(namespace_definition
  name: (namespace_name) @package.name)
"#;

const IDENTIFIER_KINDS: &[&str] = &["name", "variable_name"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("interface_declaration", "name"),
    ("trait_declaration", "name"),
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
    "conditional_expression",
    "match_expression",
];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Php,
        tree_sitter_php::LANGUAGE_PHP.into(),
        &["php"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::DottedPackage,
        &[],
    )
}

const STRING_KINDS: &[&str] = &["string", "encapsed_string"];
