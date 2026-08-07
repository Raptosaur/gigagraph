mod common;

use common::*;

#[test]
fn extracts_sql_schema_objects() {
    let file = extract_fixture("sql", "sql/schema.sql");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Tables, index, view, and function are all definitions.
    for expected in [
        "users",
        "posts",
        "idx_posts_author",
        "author_activity",
        "posts_for",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(func(&file, "users").signature, "CREATE TABLE users (");
    assert_eq!(func(&file, "users").containing_type, None);

    // CREATE FUNCTION's argument list is @func.params.
    assert_eq!(func(&file, "posts_for").param_count, 1);

    // Column names feed the semantic feature bag.
    assert_eq!(func(&file, "users").features.get("id:email"), Some(&1));
}

#[test]
fn builds_sql_table_dependency_edges() {
    let file = extract_fixture("sql", "sql/schema.sql");

    // View -> tables it selects from (FROM + JOIN), plus the aggregate it
    // invokes.
    assert_calls(func(&file, "author_activity"), &["users", "posts", "count"]);

    // Function body -> tables.
    assert_calls(func(&file, "posts_for"), &["posts", "users"]);

    // Index -> the table it covers.
    assert_calls(func(&file, "idx_posts_author"), &["posts"]);

    // REFERENCES users(id) -> foreign-key edge.
    assert_calls(func(&file, "posts"), &["users"]);

    // DEFAULT now() is an invocation inside the table definition.
    assert_calls(func(&file, "users"), &["now"]);

    // CASE in the view counts as a branch.
    assert_eq!(
        func(&file, "author_activity").features.get("flow:branches"),
        Some(&1)
    );
}

#[test]
fn sql_statements_outside_create_land_in_toplevel() {
    let file = extract_fixture("sql", "sql/schema.sql");

    // The seed INSERT is not inside any CREATE, so its table reference is
    // attributed to the synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(top.is_toplevel);
    assert!(
        has_call(top, "users"),
        "toplevel INSERT should reference users; calls: {:?}",
        top.calls
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
}
