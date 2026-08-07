mod common;

use gigagraph::extract::TOPLEVEL_NAME;
use common::*;

#[test]
fn extracts_rust_functions() {
    let file = extract_fixture("rs", "rust/service.rs");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Free fn, async fn, generic fn, impl methods, trait default method,
    // trait-impl method, nested fn.
    for expected in [
        "create_service",
        "import_names",
        "max_score",
        "new",
        "register",
        "count",
        "summary",
        "describe",
        "tally",
        "clamp",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Methods know their impl'd type; trait default methods know their trait;
    // free and nested fns have no containing type.
    assert_eq!(
        func(&file, "register").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(
        func(&file, "describe").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(
        func(&file, "summary").containing_type.as_deref(),
        Some("Describe")
    );
    assert_eq!(func(&file, "create_service").containing_type, None);
    assert_eq!(func(&file, "clamp").containing_type, None);

    // Param counts (`&self` / `&mut self` counts as a parameter).
    assert_eq!(func(&file, "new").param_count, 0);
    assert_eq!(func(&file, "register").param_count, 2);
    assert_eq!(func(&file, "max_score").param_count, 2);
    assert_eq!(func(&file, "tally").param_count, 1);

    // Call attribution: plain calls, method calls, and macros land in the
    // right function.
    assert_calls(
        func(&file, "register"),
        &["normalize", "validate_name", "insert", "eprintln"],
    );
    assert_calls(func(&file, "summary"), &["describe", "format"]);
    assert_calls(
        func(&file, "import_names"),
        &["create_service", "read_lines", "register", "push", "format"],
    );
    assert_calls(func(&file, "tally"), &["clamp", "println"]);

    // Receivers: field chains, `self`, locals, and `Type::` path scopes.
    let insert_call = func(&file, "register")
        .calls
        .iter()
        .find(|c| c.name == "insert")
        .unwrap();
    assert_eq!(insert_call.receiver.as_deref(), Some("self.users"));
    assert_eq!(insert_call.arg_count, 2);
    let describe_call = func(&file, "summary")
        .calls
        .iter()
        .find(|c| c.name == "describe")
        .unwrap();
    assert_eq!(describe_call.receiver.as_deref(), Some("self"));
    let new_call = func(&file, "create_service")
        .calls
        .iter()
        .find(|c| c.name == "new")
        .unwrap();
    assert_eq!(new_call.receiver.as_deref(), Some("UserService"));
    let register_call = func(&file, "import_names")
        .calls
        .iter()
        .find(|c| c.name == "register")
        .unwrap();
    assert_eq!(register_call.receiver.as_deref(), Some("service"));

    // Control flow: tally's for loop and match/if shape land in the bag.
    let tally_fn = func(&file, "tally");
    assert_eq!(tally_fn.features.get("flow:loops"), Some(&1));
    assert!(tally_fn.features.contains_key("flow:branches"));
    assert!(tally_fn.features.contains_key("call:clamp"));

    // Imports: full paths, grouped names, alias, wildcard.
    let paths = import_paths(&file);
    for p in [
        "std::collections::HashMap",
        "std::fmt::Write",
        "crate::validators",
        "crate::models",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let hashmap_imp = file
        .imports
        .iter()
        .find(|i| i.path == "std::collections::HashMap")
        .unwrap();
    assert!(hashmap_imp.names.contains(&"HashMap".to_string()));
    let grouped = file
        .imports
        .iter()
        .find(|i| i.path == "crate::validators")
        .unwrap();
    for n in ["normalize", "read_lines", "validate_name"] {
        assert!(
            grouped.names.contains(&n.to_string()),
            "missing grouped name {n}; got {:?}",
            grouped.names
        );
    }
    let alias = file
        .imports
        .iter()
        .find(|i| i.path == "std::fmt::Write")
        .unwrap();
    assert!(alias.names.contains(&"FmtWrite".to_string()));

    // Rust has no executable top level: no synthetic toplevel function.
    assert!(names.iter().all(|n| *n != TOPLEVEL_NAME));
}

#[test]
fn extracts_rust_di_types() {
    let file = extract_fixture("rs", "rust/wiring.rs");

    // Struct fields: plain, reference (&'static T), and the Arc/Box<dyn Trait>
    // DI idiom (inner type captured, wrapper discarded).
    assert_eq!(field_of(&file, "DbStore", "pool"), Some("ConnectionPool"));
    assert_eq!(field_of(&file, "Service", "store"), Some("DbStore"));
    assert_eq!(field_of(&file, "Service", "shared"), Some("Store"));
    assert_eq!(field_of(&file, "Service", "fallback"), Some("Store"));
    assert_eq!(field_of(&file, "Service", "config"), Some("Config"));
    // Rc<T> with a plain (non-dyn) argument also unwraps.
    assert_eq!(field_of(&file, "Service", "cache"), Some("Cache"));

    // Typed params land as locals of their function; `&self` never does.
    let ctor = func(&file, "with_store");
    assert_eq!(local_of(ctor, "store"), Some("DbStore"));
    assert_eq!(local_of(ctor, "shared"), Some("Store"));
    assert_eq!(local_of(ctor, "config"), Some("Config"));
    assert_eq!(local_of(ctor, "self"), None);

    // Locals: `let x = T::new()`, annotated `let x: T`, `let x: &T`, and
    // `let x = Arc::new(T::new())` (records the inner type, not Arc).
    let run = func(&file, "run");
    assert_eq!(local_of(run, "s"), Some("DbStore"));
    assert_eq!(local_of(run, "annotated"), Some("DbStore"));
    assert_eq!(local_of(run, "borrowed"), Some("Config"));
    assert_eq!(local_of(run, "wrapped"), Some("DbStore"));

    // `impl Store for DbStore` yields the trait->impl hierarchy edge.
    assert!(
        file.hierarchy
            .contains(&("DbStore".into(), "Store".into())),
        "missing DbStore->Store edge; got {:?}",
        file.hierarchy
    );
    // Inherent impls (`impl Service`) contribute no edge.
    assert!(file.hierarchy.iter().all(|(t, _)| t != "Service"));

    // Receiver text of the self-field call is preserved for the resolver.
    let save_call = run.calls.iter().find(|c| c.name == "save").unwrap();
    assert_eq!(save_call.receiver.as_deref(), Some("self.store"));
}

#[test]
fn extracts_rust_helpers() {
    let file = extract_fixture("rs", "rust/validators.rs");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    for expected in [
        "validate_name",
        "normalize",
        "read_lines",
        "normalize_lowercases",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Path call through a module scope keeps the scope as receiver.
    let read_call = func(&file, "read_lines")
        .calls
        .iter()
        .find(|c| c.name == "read_to_string")
        .unwrap();
    assert_eq!(read_call.receiver.as_deref(), Some("std::fs"));
    assert_calls(func(&file, "read_lines"), &["new", "map", "collect"]);

    // Fns inside a `mod` report the module as containing type.
    assert_eq!(
        func(&file, "normalize_lowercases")
            .containing_type
            .as_deref(),
        Some("tests")
    );
    assert_calls(
        func(&file, "normalize_lowercases"),
        &["normalize", "assert_eq"],
    );

    // Plain and wildcard-of-super imports.
    let paths = import_paths(&file);
    for p in ["std::path::Path", "super"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let path_imp = file
        .imports
        .iter()
        .find(|i| i.path == "std::path::Path")
        .unwrap();
    assert!(path_imp.names.contains(&"Path".to_string()));
}
