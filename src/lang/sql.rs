use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// SQL schemas via the DerekStride grammar (`tree-sitter-sequel`). Verified
// against tests/fixtures/probe/probe.sql (see tests/sql_test.rs).
//
// Mapping for a schema language: CREATE'd objects are the API surface, so
// they become `@func.def`s and table references become `@call`s, giving a
// table-dependency graph (view -> table, function -> table, index -> table).
//
// Grammar shapes (verified via dump):
// - `create_table` / `create_view` / `create_materialized_view` /
//   `create_function` name their object via a direct `object_reference` child
//   with a `name:` field. `create_index` instead puts the index name in a
//   `column:` field and references its table via a direct `object_reference`.
// - `create_function` carries a `function_arguments` list (one
//   `function_argument` per parameter) -> `@func.params`. This grammar has no
//   `create_procedure` node; procedures are not extracted.
// - Table refs: FROM/JOIN/UPDATE wrap tables in `(relation (object_reference))`;
//   INSERT and DELETE reference the table via a direct `object_reference`
//   child of `insert` / `from`. `field` nodes (`u.email`) nest their
//   `object_reference` deeper, so column refs are NOT captured as calls.
// - `REFERENCES users(id)` puts an `object_reference` directly under
//   `column_definition` -> foreign-key edge (table -> table). The pattern
//   also matches a column's `custom_type:` object_reference, which is a
//   dependency too (table -> enum/domain type).
// - Scalar/aggregate function calls are `invocation` nodes; their callee is
//   an `object_reference` too. Parameters are bare `term` fields (no argument
//   list node), so `arg_count` stays 0.
// - Statements outside any CREATE (seed/migration DML) land in the synthetic
//   `(toplevel)` function — desirable: a migration's DML edges to its tables.
// - Control flow: `case` is the only branch-like node; the grammar has no
//   loop nodes.
const QUERY: &str = r#"
(create_table (object_reference name: (identifier) @func.name)) @func.def

(create_view (object_reference name: (identifier) @func.name)) @func.def

(create_materialized_view (object_reference name: (identifier) @func.name)) @func.def

(create_function
  (object_reference name: (identifier) @func.name)
  (function_arguments) @func.params) @func.def

(create_index column: (identifier) @func.name) @func.def

(relation (object_reference name: (identifier) @call.name) @call)

(insert (object_reference name: (identifier) @call.name) @call)

(from (object_reference name: (identifier) @call.name) @call)

(create_index (object_reference name: (identifier) @call.name) @call)

(column_definition (object_reference name: (identifier) @call.name) @call)

(invocation (object_reference name: (identifier) @call.name)) @call
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier"];

// `literal` covers strings and numbers alike; no `@call.args` is captured for
// SQL, so this only matters if arg harvesting is ever wired up.
const STRING_KINDS: &[&str] = &["literal"];

const TYPE_KINDS: &[(&str, &str)] = &[];

const LOOP_KINDS: &[&str] = &[];

const BRANCH_KINDS: &[&str] = &["case"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Sql,
        tree_sitter_sequel::LANGUAGE.into(),
        &["sql"],
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
