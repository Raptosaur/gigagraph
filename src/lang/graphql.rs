use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// GraphQL SDL (`.graphql` / `.gql`) via `tree-sitter-graphql` (the standard
// GraphQL grammar). Verified against tests/fixtures/probe/probe.graphql (see
// tests/graphql_test.rs).
//
// Mapping for a schema language: type / interface / input / enum / union /
// scalar definitions are the API surface -> `@func.def` named by the type.
// Every `named_type` usage (field types, `implements` clauses, union
// members, argument types) becomes a `@call` to that type name, yielding a
// type-dependency graph (User -> Post, Query -> SearchResult, User -> Node).
// Built-in scalars (`ID`, `String`, ...) produce unresolved calls, which is
// fine. Directives and executable documents (queries/mutations/fragments)
// are deliberately ignored in phase 1.
//
// Grammar shapes (verified via dump): this grammar has no fields; the
// definition's own `name` is its only *direct* `name` child (union members /
// implemented interfaces nest inside `union_member_types` /
// `implements_interfaces`), so `(x_type_definition (name) @func.name)` is
// unambiguous.
const QUERY: &str = r#"
(object_type_definition (name) @func.name) @func.def

(interface_type_definition (name) @func.name) @func.def

(input_object_type_definition (name) @func.name) @func.def

(enum_type_definition (name) @func.name) @func.def

(union_type_definition (name) @func.name) @func.def

(scalar_type_definition (name) @func.name) @func.def

(named_type (name) @call.name) @call
"#;

const IDENTIFIER_KINDS: &[&str] = &["name"];

const STRING_KINDS: &[&str] = &["string_value"];

const TYPE_KINDS: &[(&str, &str)] = &[];

const LOOP_KINDS: &[&str] = &[];

const BRANCH_KINDS: &[&str] = &[];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Graphql,
        tree_sitter_graphql::LANGUAGE.into(),
        &["graphql", "gql"],
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
