use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Validated against tests/fixtures/java/*.java (see tests/java_test.rs).
//
// Notes:
// - `method_invocation` is matched by a single pattern with an optional
//   `object:` capture. Two separate patterns (with/without object) would both
//   match receiver calls and the extractor's dedupe keeps whichever match
//   arrives first — which could drop `@call.recv`. One pattern avoids that.
// - Plain imports anchor the `scoped_identifier` as the last named child so
//   the wildcard form (`import java.util.*;`, which has a trailing
//   `(asterisk)`) doesn't bind its final package segment as an import name.
// - `package` accepts both multi-segment (`scoped_identifier`) and
//   single-segment (`identifier`) declarations.
// - A record's compact constructor (`record R(int x) { R { ... } }`) has no
//   parameter list of its own; the pattern reaches up to the enclosing
//   `record_declaration` so the record header supplies `@func.params`.
// - The class-level `@RequestMapping` pattern deliberately captures NEITHER
//   `@deco.type` (that would route it to `ExtractedFile.type_decorations`,
//   which endpoint detection never sees) NOR `@func.def`: with no same-match
//   definition the extractor falls back to nearest-following-function
//   association, attaching the annotation to the class's first method — the
//   endpoint pre-pass recovers the prefix from there via `containing_type`
//   (same ride-along trick the TS query uses for NestJS `@Controller`).
//   Known limit: a mapped class with no methods leaks the annotation to the
//   next function in the file. Gated to `RequestMapping` so ordinary class
//   annotations (`@Entity(...)`) don't pollute the decoration stream or set
//   `has_decorations` on unrelated methods.
const QUERY: &str = r#"
(method_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  (modifiers
    (annotation
      name: (identifier) @deco.name
      arguments: (annotation_argument_list) @deco.args) @deco)
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  (modifiers
    (marker_annotation
      name: (identifier) @deco.name) @deco)
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

((class_declaration
  (modifiers
    (annotation
      name: (identifier) @deco.name
      arguments: (annotation_argument_list) @deco.args) @deco))
  (#eq? @deco.name "RequestMapping"))

(constructor_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(record_declaration
  parameters: (formal_parameters) @func.params
  body: (class_body
    (compact_constructor_declaration
      name: (identifier) @func.name) @func.def))

(method_invocation
  object: (_)? @call.recv
  name: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (type_identifier) @call.name
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (generic_type (type_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (scoped_type_identifier (type_identifier) @call.name .)
  arguments: (argument_list) @call.args) @call

(import_declaration
  (scoped_identifier
    name: (identifier) @import.name) @import.path .) @import

(import_declaration
  (scoped_identifier) @import.path
  (asterisk)) @import

(package_declaration (scoped_identifier) @package.name)

(package_declaration (identifier) @package.name)

(field_declaration
  declarator: (variable_declarator
    name: (identifier) @const.name
    value: (string_literal) @const.value))

(return_statement (string_literal) @ret.str)
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "type_identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("interface_declaration", "name"),
    ("enum_declaration", "name"),
    ("record_declaration", "name"),
    ("annotation_type_declaration", "name"),
];

const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "enhanced_for_statement",
    "while_statement",
    "do_statement",
];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_expression",
    "ternary_expression",
    "catch_clause",
];

const BUILTIN_RECEIVERS: &[&str] = &["System", "Math", "Objects", "Arrays", "Collections"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Java,
        tree_sitter_java::LANGUAGE.into(),
        &["java"],
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

const STRING_KINDS: &[&str] = &["string_literal"];
