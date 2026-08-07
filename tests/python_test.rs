mod common;

use common::*;

#[test]
fn extracts_python_functions() {
    let file = extract_fixture("py", "python/service.py");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Module-level def, async def, dunder method, plain/decorated/async
    // methods, nested def, subclass method.
    for expected in [
        "make_service",
        "warm_cache",
        "__init__",
        "register",
        "validate",
        "build_user",
        "merge",
        "count",
        "refresh",
        "export",
        "render",
        "promote",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Methods know their class; module functions don't have one. `render` is
    // a def nested inside a method — its nearest class ancestor is still the
    // enclosing class, so it reports UserService too.
    assert_eq!(
        func(&file, "register").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(
        func(&file, "refresh").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(
        func(&file, "promote").containing_type.as_deref(),
        Some("AdminService")
    );
    assert_eq!(
        func(&file, "render").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(func(&file, "make_service").containing_type, None);

    // Decorated defs capture the inner function_definition, so the signature
    // is the `def` line, not the `@decorator` line.
    assert!(func(&file, "merge").signature.starts_with("def merge"));
    assert!(
        func(&file, "warm_cache")
            .signature
            .starts_with("async def warm_cache")
    );

    // Param counts: `self` is a named child of `parameters`, so it is
    // INCLUDED in the count. A `x=None` default counts as one parameter.
    assert_eq!(func(&file, "register").param_count, 3); // self, email, name
    assert_eq!(func(&file, "merge").param_count, 2); // base, extra=None
    assert_eq!(func(&file, "count").param_count, 1); // self
    assert_eq!(func(&file, "make_service").param_count, 1);

    // Call sites attributed to the right function, receivers captured.
    assert_calls(
        func(&file, "make_service"),
        &["load", "join", "UserService"],
    );
    assert_calls(
        func(&file, "register"),
        &["normalize_email", "validate", "error", "build_user", "emit"],
    );
    let validate_call = func(&file, "register")
        .calls
        .iter()
        .find(|c| c.name == "validate")
        .unwrap();
    assert_eq!(validate_call.receiver.as_deref(), Some("self"));
    let build_user_call = func(&file, "register")
        .calls
        .iter()
        .find(|c| c.name == "build_user")
        .unwrap();
    assert_eq!(build_user_call.receiver.as_deref(), Some("self"));
    assert_eq!(build_user_call.arg_count, 2);
    let error_call = func(&file, "register")
        .calls
        .iter()
        .find(|c| c.name == "error")
        .unwrap();
    assert_eq!(error_call.receiver.as_deref(), Some("log"));

    // Dotted receiver text is kept verbatim.
    let join_call = func(&file, "make_service")
        .calls
        .iter()
        .find(|c| c.name == "join")
        .unwrap();
    assert_eq!(join_call.receiver.as_deref(), Some("os.path"));

    // `await service.refresh()` — receiver survives the await wrapper.
    let refresh_call = func(&file, "warm_cache")
        .calls
        .iter()
        .find(|c| c.name == "refresh")
        .unwrap();
    assert_eq!(refresh_call.receiver.as_deref(), Some("service"));

    // `super().build_user(...)`: the callee is kept but the `super()`
    // receiver text contains parens and is dropped by the sanitizer.
    assert_calls(func(&file, "promote"), &["super", "build_user"]);
    let super_call = func(&file, "promote")
        .calls
        .iter()
        .find(|c| c.name == "build_user")
        .unwrap();
    assert_eq!(super_call.receiver, None);

    // Control flow: refresh has a for + while; export's list comprehension
    // counts via for_in_clause; register's `if` is a branch.
    let refresh_fn = func(&file, "refresh");
    assert_eq!(refresh_fn.features.get("flow:loops"), Some(&2));
    assert!(refresh_fn.features.contains_key("call:dumps"));
    assert_eq!(func(&file, "export").features.get("flow:loops"), Some(&1));
    assert_eq!(
        func(&file, "register").features.get("flow:branches"),
        Some(&1)
    );

    // The `render(u)` call inside the comprehension belongs to `export`,
    // not to the nested `render` def.
    assert_calls(func(&file, "export"), &["render", "values"]);
    assert!(!has_call(func(&file, "render"), "render"));

    // Top-level `make_service(".")` becomes the synthetic toplevel
    // function's call.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "make_service"));

    // Imports: plain, dotted, aliased, from-import (plain + aliased),
    // relative (leading dots preserved).
    let paths = import_paths(&file);
    for p in [
        "json",
        "os.path",
        "logging",
        "collections",
        "util",
        ".",
        "..core",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let log_imp = file.imports.iter().find(|i| i.path == "logging").unwrap();
    assert!(log_imp.names.contains(&"log".to_string()));
    let coll_imp = file
        .imports
        .iter()
        .find(|i| i.path == "collections")
        .unwrap();
    assert!(coll_imp.names.contains(&"OrderedDict".to_string()));
    let util_imp = file.imports.iter().find(|i| i.path == "util").unwrap();
    assert!(util_imp.names.contains(&"normalize_email".to_string()));
    assert!(util_imp.names.contains(&"load".to_string()));
    let rel_imp = file.imports.iter().find(|i| i.path == ".").unwrap();
    assert!(rel_imp.names.contains(&"repo".to_string()));
    let core_imp = file.imports.iter().find(|i| i.path == "..core").unwrap();
    assert!(core_imp.names.contains(&"events".to_string()));
}

#[test]
fn extracts_python_util_shapes() {
    let file = extract_fixture("py", "python/util.py");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    for expected in ["normalize_email", "load_settings", "slugify"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Typed parameter (`text: str`) still counts as one param.
    assert_eq!(func(&file, "slugify").param_count, 1);

    // Chained call `email.strip().lower()`: both links captured; the outer
    // receiver text contains `(` so it is dropped.
    assert_calls(func(&file, "normalize_email"), &["strip", "lower"]);
    let strip_call = func(&file, "normalize_email")
        .calls
        .iter()
        .find(|c| c.name == "strip")
        .unwrap();
    assert_eq!(strip_call.receiver.as_deref(), Some("email"));
    let lower_call = func(&file, "normalize_email")
        .calls
        .iter()
        .find(|c| c.name == "lower")
        .unwrap();
    assert_eq!(lower_call.receiver, None);

    assert_calls(func(&file, "load_settings"), &["open", "load"]);
    let json_load = func(&file, "load_settings")
        .calls
        .iter()
        .find(|c| c.name == "load")
        .unwrap();
    assert_eq!(json_load.receiver.as_deref(), Some("json"));

    let sub_call = func(&file, "slugify")
        .calls
        .iter()
        .find(|c| c.name == "sub")
        .unwrap();
    assert_eq!(sub_call.receiver.as_deref(), Some("re"));
    assert_eq!(sub_call.arg_count, 3);

    let paths = import_paths(&file);
    for p in ["json", "re"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
}

#[test]
fn docstrings_feed_doc_features() {
    let src = "def summarize(rows):\n    \"\"\"Aggregate rows into weekly buckets.\"\"\"\n    return rows\n";
    let file = extract_str("py", src);
    let f = func(&file, "summarize");
    for tok in ["doc:aggregate", "doc:weekly", "doc:buckets"] {
        assert!(f.features.contains_key(tok), "missing {tok}");
    }
}
