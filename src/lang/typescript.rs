use super::LangSpec;
use crate::lang::javascript;
use crate::types::{ImportStyle, Lang};

/// TS-only constructs on top of the shared JS/TS core.
///
/// Decorators (verified via `cargo run --example dump`): method decorators
/// are `decorator` SIBLINGS preceding the `method_definition` inside
/// `class_body`, and class decorators precede the `class_declaration`
/// (inside `export_statement` or at statement level). Both therefore end
/// before the next `@func.def` starts, so the extractor's
/// nearest-following-function association attaches method decorators to
/// their method and class decorators to the class's first method — which
/// `src/endpoints.rs` exploits (via `containing_type`) to recover NestJS
/// `@Controller` prefixes. Deliberately NO `@deco.type` capture: that would
/// route class decorators to `type_decorations`, which endpoint detection
/// does not receive.
///
/// Type captures (verified via probe dump):
/// - `type_annotation` wraps a `type_identifier` for plain names and a
///   `nested_type_identifier` (fields `module`/`name`) for `cdk.App`-style
///   ones; the alternation captures either whole — `clean_type` keeps the
///   last dotted segment. Generics/unions land as other node kinds the
///   alternation skips.
/// - Ctor parameter properties: `private x: T` is a `required_parameter`
///   with an `accessibility_modifier` child, but bare `readonly x: T` has NO
///   named modifier child — the keyword is an anonymous `"readonly"` token,
///   hence the quoted-token pattern. `x?: T` variants are a distinct
///   `optional_parameter` kind. Param properties also match the plain
///   `@local` param pattern (deliberate, like PHP promoted props: the name
///   is usable in the ctor body); the extractor dedups the field rows.
/// - `this.x = new T()` joins cover TS ctors that assign without a typed
///   declaration; when both exist the field row dedups.
/// - Heritage: TS class names are `type_identifier` but an `extends_clause`
///   value is an `identifier` (or `member_expression` for `cdk.Stack`);
///   `implements_clause` holds bare `type_identifier`s. All patterns need
///   duplicating for `abstract_class_declaration` — it is a distinct node
///   kind, not a modifier on `class_declaration`. Interface extends is its
///   own `extends_type_clause` with a `type:` field.
const TS_EXTRA: &str = r#"
(public_field_definition
  name: (property_identifier) @func.name
  value: [(arrow_function) (function_expression)] @func.body) @func.def

(import_statement
  (import_require_clause
    (identifier) @import.name
    source: (string (string_fragment) @import.path))) @import

(decorator
  (call_expression
    function: (identifier) @deco.name
    arguments: (arguments) @deco.args)) @deco

(public_field_definition
  name: (property_identifier) @field.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @field.type))

(required_parameter
  (accessibility_modifier)
  pattern: (identifier) @field.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @field.type))

(required_parameter
  "readonly"
  pattern: (identifier) @field.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @field.type))

(optional_parameter
  (accessibility_modifier)
  pattern: (identifier) @field.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @field.type))

(assignment_expression
  left: (member_expression
    object: (this)
    property: (property_identifier) @field.name)
  right: (new_expression constructor: (identifier) @field.type))

(required_parameter
  pattern: (identifier) @local.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @local.type))

(optional_parameter
  pattern: (identifier) @local.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @local.type))

(variable_declarator
  name: (identifier) @local.name
  type: (type_annotation [(type_identifier) (nested_type_identifier)] @local.type))

(variable_declarator
  name: (identifier) @local.name
  value: (new_expression constructor: (identifier) @local.type))

(class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (extends_clause value: (identifier) @hier.base)))

(class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (extends_clause value: (member_expression property: (property_identifier) @hier.base))))

(class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (implements_clause [(type_identifier) (nested_type_identifier)] @hier.base)))

(abstract_class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (extends_clause value: (identifier) @hier.base)))

(abstract_class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (extends_clause value: (member_expression property: (property_identifier) @hier.base))))

(abstract_class_declaration
  name: (type_identifier) @hier.type
  (class_heritage (implements_clause [(type_identifier) (nested_type_identifier)] @hier.base)))

(interface_declaration
  name: (type_identifier) @hier.type
  (extends_type_clause type: [(type_identifier) (nested_type_identifier)] @hier.base))
"#;

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("class", "name"),
    ("abstract_class_declaration", "name"),
    ("interface_declaration", "name"),
    ("enum_declaration", "name"),
    ("internal_module", "name"),
];

fn ts_query() -> String {
    format!("{}{}", javascript::CORE_QUERY, TS_EXTRA)
}

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::TypeScript,
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &["ts", "mts", "cts"],
        &ts_query(),
        javascript::IDENTIFIER_KINDS,
        javascript::STRING_KINDS,
        TYPE_KINDS,
        javascript::LOOP_KINDS,
        javascript::BRANCH_KINDS,
        ImportStyle::PathLike,
        javascript::BUILTIN_RECEIVERS,
    )
}

pub fn spec_tsx() -> LangSpec {
    LangSpec::new(
        Lang::Tsx,
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        &["tsx"],
        &ts_query(),
        javascript::IDENTIFIER_KINDS,
        javascript::STRING_KINDS,
        TYPE_KINDS,
        javascript::LOOP_KINDS,
        javascript::BRANCH_KINDS,
        ImportStyle::PathLike,
        javascript::BUILTIN_RECEIVERS,
    )
}
