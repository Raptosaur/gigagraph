mod common;

use common::*;

#[test]
fn extracts_kotlin_functions() {
    let file = extract_fixture("kt", "kotlin/service.kt");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Top-level fn, methods, local (nested) fn, lambda-using method, suspend
    // method, companion-object fn, object-declaration fn, data-class method,
    // extension fn, infix fn, operator fn.
    for expected in [
        "createUser",
        "normalize",
        "register",
        "validate",
        "hasDomain",
        "renderAll",
        "render",
        "refresh",
        "default",
        "lookup",
        "save",
        "initials",
        "clampTo",
        "plus",
        "retryTimes",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Multi-segment package header.
    assert_eq!(file.package.as_deref(), Some("com.example.service"));

    // Methods know their class/object; top-level and extension fns don't.
    assert_eq!(
        func(&file, "register").containing_type.as_deref(),
        Some("UserService")
    );
    // Local function nested in a method still resolves to the class.
    assert_eq!(
        func(&file, "hasDomain").containing_type.as_deref(),
        Some("UserService")
    );
    // Companion-object function resolves to the enclosing class.
    assert_eq!(
        func(&file, "default").containing_type.as_deref(),
        Some("UserService")
    );
    // Object declaration member.
    assert_eq!(
        func(&file, "lookup").containing_type.as_deref(),
        Some("Registry")
    );
    // Data-class method.
    assert_eq!(
        func(&file, "save").containing_type.as_deref(),
        Some("UserRepo")
    );
    assert_eq!(func(&file, "createUser").containing_type, None);
    assert_eq!(func(&file, "initials").containing_type, None);

    // Param counts (extension/infix receivers are not parameters).
    assert_eq!(func(&file, "register").param_count, 2);
    assert_eq!(func(&file, "normalize").param_count, 1);
    assert_eq!(func(&file, "refresh").param_count, 0);
    assert_eq!(func(&file, "retryTimes").param_count, 2);
    assert_eq!(func(&file, "clampTo").param_count, 1);
    assert_eq!(func(&file, "initials").param_count, 0);

    // Plain call + constructor-style call.
    assert_calls(func(&file, "createUser"), &["normalize", "User"]);
    let user_ctor = func(&file, "createUser")
        .calls
        .iter()
        .find(|c| c.name == "User")
        .unwrap();
    assert_eq!(user_ctor.arg_count, 2);

    // Chained calls: first link keeps its receiver, later links drop the
    // parenthesized receiver text.
    let normalize_fn = func(&file, "normalize");
    assert_calls(normalize_fn, &["trim", "lowercase"]);
    let trim_call = normalize_fn
        .calls
        .iter()
        .find(|c| c.name == "trim")
        .unwrap();
    assert_eq!(trim_call.receiver.as_deref(), Some("email"));
    let lower_call = normalize_fn
        .calls
        .iter()
        .find(|c| c.name == "lowercase")
        .unwrap();
    assert_eq!(lower_call.receiver, None);

    // Receiver forms: this.m(), obj.m().
    let register_fn = func(&file, "register");
    assert_calls(register_fn, &["createUser", "validate", "save"]);
    let validate_call = register_fn
        .calls
        .iter()
        .find(|c| c.name == "validate")
        .unwrap();
    assert_eq!(validate_call.receiver.as_deref(), Some("this"));
    let save_call = register_fn.calls.iter().find(|c| c.name == "save").unwrap();
    assert_eq!(save_call.receiver.as_deref(), Some("repo"));
    let create_call = register_fn
        .calls
        .iter()
        .find(|c| c.name == "createUser")
        .unwrap();
    assert_eq!(create_call.arg_count, 2);

    // Nested-function attribution: the local fn owns its own calls.
    assert_calls(func(&file, "validate"), &["hasDomain"]);
    assert!(
        !has_call(func(&file, "validate"), "contains"),
        "call inside local fn leaked to the enclosing method"
    );
    assert_calls(func(&file, "hasDomain"), &["contains"]);

    // Calls inside lambda arguments attribute to the enclosing function.
    let render_all = func(&file, "renderAll");
    assert_calls(render_all, &["map", "render"]);
    let map_call = render_all.calls.iter().find(|c| c.name == "map").unwrap();
    assert_eq!(map_call.receiver.as_deref(), Some("users"));

    // Aliased receiver from an `as` import.
    let pretty_call = func(&file, "render")
        .calls
        .iter()
        .find(|c| c.name == "pretty")
        .unwrap();
    assert_eq!(pretty_call.receiver.as_deref(), Some("Fmt"));

    // Companion function calling constructors.
    assert_calls(func(&file, "default"), &["UserService", "UserRepo"]);

    // Extension function: `this` receiver, trailing-lambda call, lambda body
    // call attributed here.
    let initials_fn = func(&file, "initials");
    assert_calls(initials_fn, &["split", "map", "first", "joinToString"]);
    let split_call = initials_fn
        .calls
        .iter()
        .find(|c| c.name == "split")
        .unwrap();
    assert_eq!(split_call.receiver.as_deref(), Some("this"));

    // Operator fn body.
    assert_calls(func(&file, "plus"), &["save"]);

    // Control flow features.
    let retry_fn = func(&file, "retryTimes");
    assert_calls(retry_fn, &["block", "println"]);
    assert_eq!(retry_fn.features.get("flow:loops"), Some(&1));
    assert_eq!(retry_fn.features.get("flow:branches"), Some(&1)); // catch
    assert_eq!(func(&file, "refresh").features.get("flow:loops"), Some(&1));
    assert_eq!(
        func(&file, "clampTo").features.get("flow:branches"),
        Some(&1)
    );

    // Top-level property initializer call lands in the synthetic toplevel fn.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "default"));

    // Imports: plain, aliased, wildcard.
    let paths = import_paths(&file);
    for p in [
        "com.example.model.User",
        "com.example.util.Formatter",
        "kotlinx.coroutines",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let user_imp = file
        .imports
        .iter()
        .find(|i| i.path == "com.example.model.User")
        .unwrap();
    assert!(user_imp.names.contains(&"User".to_string()));
    assert!(!user_imp.system);
    let alias_imp = file
        .imports
        .iter()
        .find(|i| i.path == "com.example.util.Formatter")
        .unwrap();
    assert!(
        alias_imp.names.contains(&"Fmt".to_string()),
        "alias should be the bound name; got {:?}",
        alias_imp.names
    );
    assert!(!alias_imp.names.contains(&"Formatter".to_string()));
    let star_imp = file
        .imports
        .iter()
        .find(|i| i.path == "kotlinx.coroutines")
        .unwrap();
    assert!(
        star_imp.names.is_empty(),
        "wildcard import binds no names; got {:?}",
        star_imp.names
    );
}

#[test]
fn extracts_kotlin_models() {
    let file = extract_fixture("kt", "kotlin/models.kt");

    // Single-segment package header.
    assert_eq!(file.package.as_deref(), Some("models"));

    assert_eq!(
        func(&file, "record").containing_type.as_deref(),
        Some("AuditLog")
    );
    let record_fn = func(&file, "record");
    assert_calls(record_fn, &["add", "format"]);
    let add_call = record_fn.calls.iter().find(|c| c.name == "add").unwrap();
    assert_eq!(add_call.receiver.as_deref(), Some("entries"));

    let format_fn = func(&file, "format");
    assert_calls(format_fn, &["now", "toString"]);
    let now_call = format_fn.calls.iter().find(|c| c.name == "now").unwrap();
    assert_eq!(now_call.receiver.as_deref(), Some("Instant"));
    // `when` expression counts as a branch.
    assert_eq!(format_fn.features.get("flow:branches"), Some(&1));

    let imp = file
        .imports
        .iter()
        .find(|i| i.path == "java.time.Instant")
        .unwrap();
    assert!(imp.names.contains(&"Instant".to_string()));
}

#[test]
fn annotated_function_signature_skips_annotations() {
    let src = r#"
class Probe {
    @Test
    @Suppress(
        "unused",
    )
    fun check(): Int {
        return 1
    }

    @JvmStatic fun inline(): Int = 2
}
"#;
    let file = extract_str("kt", src);
    // Multi-line annotations (with wrapped argument lists) are skipped.
    assert_eq!(func(&file, "check").signature, "fun check(): Int {");
    // Annotation and declaration on one line: keep the whole line.
    assert_eq!(
        func(&file, "inline").signature,
        "@JvmStatic fun inline(): Int = 2"
    );
}
