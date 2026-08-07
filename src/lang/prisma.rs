use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Prisma schemas (`schema.prisma`) via `tree-sitter-prisma-io`. Verified
// against tests/fixtures/probe/probe.prisma (see tests/prisma_test.rs).
//
// Mapping for a schema language: model/enum/type/view declarations are the
// API surface -> `@func.def` named by the declaration. A column's type
// reference becomes a `@call` to that type name: relation fields
// (`author User`, `posts Post[]`) resolve to the referenced model, giving
// model -> model edges. Scalar types (`Int`, `String`, `DateTime`) produce
// unresolved calls — a pure query cannot tell scalars from models; resolution
// happens (or fails, harmlessly) at graph-build time.
//
// Grammar shapes (verified via dump):
// - `model_declaration` / `enum_declaration` / `view_declaration` are
//   `(identifier, body-block)`; `type_declaration` adds more children but the
//   name is still the direct `identifier` child. Column and enumeral
//   identifiers sit inside the block, never as direct children.
// - `column_declaration` is `(identifier, column_type, attribute*)`; the
//   column's type name is the `identifier` inside `column_type` (`Post[]`
//   adds an `array` sibling).
// - Attribute/expression calls (`@default(autoincrement())`, `env("URL")`)
//   are `call_expression (identifier, arguments)`; `env(...)` in datasource
//   blocks lands in the synthetic `(toplevel)` function since datasource /
//   generator blocks are deliberately not `@func.def`s (config, not schema).
const QUERY: &str = r#"
(model_declaration (identifier) @func.name) @func.def

(enum_declaration (identifier) @func.name) @func.def

(type_declaration (identifier) @func.name) @func.def

(view_declaration (identifier) @func.name) @func.def

(column_declaration (column_type (identifier) @call.name) @call)

(call_expression (identifier) @call.name (arguments) @call.args) @call
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "variable", "property_identifier"];

const STRING_KINDS: &[&str] = &["string"];

const TYPE_KINDS: &[(&str, &str)] = &[];

const LOOP_KINDS: &[&str] = &[];

const BRANCH_KINDS: &[&str] = &[];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Prisma,
        tree_sitter_prisma_io::LANGUAGE.into(),
        &["prisma"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        &[],
    )
}
