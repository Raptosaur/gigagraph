mod common;

use common::*;

#[test]
fn extracts_prisma_declarations() {
    let file = extract_fixture("prisma", "prisma/schema.prisma");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    for expected in ["User", "Profile", "Post", "Role"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // datasource / generator blocks are config, not schema declarations.
    assert!(!names.contains(&"db"), "datasource captured; got {names:?}");
    assert!(
        !names.contains(&"client"),
        "generator captured; got {names:?}"
    );

    assert_eq!(func(&file, "User").signature, "model User {");
    assert_eq!(func(&file, "User").containing_type, None);

    // Field names feed the semantic feature bag.
    assert_eq!(func(&file, "User").features.get("id:email"), Some(&1));
}

#[test]
fn builds_prisma_relation_edges() {
    let file = extract_fixture("prisma", "prisma/schema.prisma");

    // Field type references edge model -> model / model -> enum. Scalar
    // types (Int, String, ...) also surface as calls but stay unresolved.
    assert_calls(func(&file, "User"), &["Post", "Profile", "Role"]);
    assert_calls(func(&file, "Post"), &["User"]);
    assert_calls(func(&file, "Profile"), &["User"]);

    // Attribute expressions are calls too (@default(autoincrement()),
    // @default(now())).
    assert_calls(func(&file, "User"), &["autoincrement"]);
    assert_calls(func(&file, "Post"), &["now"]);
}

#[test]
fn prisma_datasource_calls_land_in_toplevel() {
    let file = extract_fixture("prisma", "prisma/schema.prisma");

    // env("DATABASE_URL") sits in the datasource block, outside any
    // declaration -> synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "env"));
    let env = top.calls.iter().find(|c| c.name == "env").unwrap();
    assert_eq!(env.arg_count, 1);
}
