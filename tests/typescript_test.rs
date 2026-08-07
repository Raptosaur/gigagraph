mod common;

use common::*;

#[test]
fn extracts_typescript_functions() {
    let file = extract_fixture("ts", "typescript/service.ts");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Plain, async, method, private method, class-property arrow, const arrow.
    for expected in [
        "createServer",
        "loadConfig",
        "registerUser",
        "buildUser",
        "fetchAll",
        "retry",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Methods know their class.
    assert_eq!(
        func(&file, "registerUser").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(
        func(&file, "fetchAll").containing_type.as_deref(),
        Some("UserService")
    );
    assert_eq!(func(&file, "createServer").containing_type, None);

    // Param counts.
    assert_eq!(func(&file, "registerUser").param_count, 2);
    assert_eq!(func(&file, "loadConfig").param_count, 1);
    assert_eq!(func(&file, "fetchAll").param_count, 0);

    // Call sites attributed to the right function, receivers captured.
    assert_calls(func(&file, "createServer"), &["express", "listen"]);
    assert_calls(func(&file, "loadConfig"), &["resolve", "readFile"]);
    assert_calls(
        func(&file, "registerUser"),
        &[
            "normalizeEmail",
            "validateUser",
            "buildUser",
            "set",
            "error",
        ],
    );
    let build_user_call = func(&file, "registerUser")
        .calls
        .iter()
        .find(|c| c.name == "buildUser")
        .unwrap();
    assert_eq!(build_user_call.receiver.as_deref(), Some("this"));
    assert_eq!(build_user_call.arg_count, 2);

    // retry's loop/branch shape lands in the feature bag.
    let retry_fn = func(&file, "retry");
    assert_eq!(retry_fn.features.get("flow:loops"), Some(&1));
    assert!(retry_fn.features.contains_key("call:fn"));

    // Top-level `retry(() => createServer(8080))` becomes the synthetic
    // toplevel function's calls.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "retry"));

    // Imports: external vs relative, bound names.
    let paths = import_paths(&file);
    for p in ["express", "fs/promises", "path", "./validators", "./models"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let fs_imp = file
        .imports
        .iter()
        .find(|i| i.path == "fs/promises")
        .unwrap();
    assert!(fs_imp.names.contains(&"readFile".to_string()));
    let star = file.imports.iter().find(|i| i.path == "path").unwrap();
    assert!(star.names.contains(&"path".to_string()));
}

#[test]
fn extracts_tsx_components() {
    let file = extract_fixture("tsx", "typescript/dashboard.tsx");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Function component, arrow component, class-property arrow, method,
    // plain helper.
    for expected in [
        "Dashboard",
        "Badge",
        "refresh",
        "buildLegacy",
        "legacyMarkup",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(
        func(&file, "refresh").containing_type.as_deref(),
        Some("PanelController")
    );

    // Hook calls and JSX-attribute callbacks land in the component's calls.
    assert_calls(
        func(&file, "Dashboard"),
        &[
            "useState",
            "useEffect",
            "setInterval",
            "clearInterval",
            "setCount",
            "formatCount",
        ],
    );
    assert_calls(func(&file, "Badge"), &["onReset"]);
    assert_calls(func(&file, "buildLegacy"), &["legacyMarkup"]);

    // JSX element usage is not a call: <Dashboard/> inside refresh must not
    // fabricate a call edge to Dashboard, but render(...) is real.
    assert!(has_call(func(&file, "refresh"), "render"));
    assert!(!has_call(func(&file, "refresh"), "Dashboard"));

    let paths = import_paths(&file);
    for p in ["react", "./validators"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
}

#[test]
fn extracts_javascript_shapes() {
    let file = extract_fixture("js", "javascript/util.js");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Declaration, arrow, object-literal method (shorthand + function expr),
    // class method, class-property arrow, prototype assignment.
    for expected in [
        "readJson",
        "double",
        "triple",
        "quadruple",
        "increment",
        "reset",
        "decrement",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(
        func(&file, "increment").containing_type.as_deref(),
        Some("Counter")
    );
    assert_eq!(func(&file, "double").param_count, 1);

    assert_calls(func(&file, "readJson"), &["readFileSync", "join", "parse"]);
    assert_calls(func(&file, "quadruple"), &["double"]);

    // require() imports with bound names.
    let fs_imp = file.imports.iter().find(|i| i.path == "fs").unwrap();
    assert!(fs_imp.names.contains(&"fs".to_string()));
    assert!(import_paths(&file).contains(&"path"));
}

#[test]
fn comments_feed_doc_features() {
    let src = "// Retry with exponential backoff.\nfunction retryBackoff(): void {\n  // jitter the delay window\n  const x = 1;\n}\n";
    let file = extract_str("ts", src);
    let f = func(&file, "retryBackoff");
    for tok in ["doc:exponential", "doc:backoff", "doc:jitter"] {
        assert!(f.features.contains_key(tok), "missing {tok}");
    }
    // Short and numeric tokens are dropped; doc tf is capped at 1.
    assert!(!f.features.contains_key("doc:x"));
    assert_eq!(f.features.get("doc:the"), Some(&1));
}
