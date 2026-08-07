mod common;

use common::*;

#[test]
fn yaml_top_level_sections_become_functions() {
    let file = extract_fixture("yaml", "openapi/petstore.yaml");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Exactly the document's top-level keys, in order — nothing nested.
    assert_eq!(names, ["openapi", "info", "paths", "components"]);
    assert!(!names.contains(&"/pets"));
    assert!(!names.contains(&"get"));

    let paths = func(&file, "paths");
    assert_eq!(paths.signature, "paths:");

    // Scalars inside a section feed the semantic feature bag, so endpoint
    // paths / operation ids / schema names are searchable.
    assert_eq!(paths.features.get("id:/pets"), Some(&1));
    assert_eq!(paths.features.get("id:listPets"), Some(&1));
    assert_eq!(func(&file, "components").features.get("id:Pet"), Some(&1));
}

#[test]
fn yaml_produces_no_call_edges() {
    let file = extract_fixture("yaml", "openapi/petstore.yaml");

    // Shallow by design: YAML keys (get, post, name, ...) must never create
    // name-resolved edges into real code.
    assert!(file.functions.iter().all(|f| f.calls.is_empty()));
    assert!(file.imports.is_empty());
}
