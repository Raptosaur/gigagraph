mod common;

use common::*;

#[test]
fn extracts_graphql_type_definitions() {
    let file = extract_fixture("graphql", "graphql/schema.graphql");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // type / interface / input / enum / union definitions all extract.
    for expected in [
        "User",
        "Post",
        "Node",
        "PostInput",
        "Role",
        "SearchResult",
        "Query",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(func(&file, "Query").signature, "type Query {");
    assert_eq!(func(&file, "Query").containing_type, None);

    // Field names feed the semantic feature bag.
    assert_eq!(func(&file, "User").features.get("id:email"), Some(&1));
}

#[test]
fn builds_graphql_type_dependency_edges() {
    let file = extract_fixture("graphql", "graphql/schema.graphql");

    // Field types + implements clause. Built-in scalars (ID, String) also
    // surface as calls but stay unresolved.
    assert_calls(func(&file, "User"), &["Node", "Post", "ID", "String"]);
    assert_calls(func(&file, "Post"), &["User"]);

    // Union members reference their variants.
    assert_calls(func(&file, "SearchResult"), &["User", "Post"]);

    // Root type references both return types and argument types.
    assert_calls(
        func(&file, "Query"),
        &["User", "SearchResult", "ID", "String"],
    );

    // Every named_type sits inside a definition; no toplevel synthetic
    // function should appear.
    assert!(file.functions.iter().all(|f| !f.is_toplevel));
}
