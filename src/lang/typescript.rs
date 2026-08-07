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
