mod common;

use common::*;

#[test]
fn extracts_php_service() {
    let file = extract_fixture("php", "php/Service.php");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Constructor, instance methods, private method, static method.
    for expected in ["__construct", "notify", "render", "deliver"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Namespace lands in the package field.
    assert_eq!(file.package.as_deref(), Some("App\\Services"));

    // Methods know their class; the interface signature knows its interface.
    assert_eq!(
        func(&file, "__construct").containing_type.as_deref(),
        Some("MailerService")
    );
    assert_eq!(
        func(&file, "render").containing_type.as_deref(),
        Some("MailerService")
    );
    assert_eq!(
        func(&file, "deliver").containing_type.as_deref(),
        Some("MailerService")
    );
    assert!(
        file.functions
            .iter()
            .any(|f| f.name == "notify" && f.containing_type.as_deref() == Some("Notifier")),
        "interface method signature should be extracted with containing_type Notifier"
    );
    let notify = file
        .functions
        .iter()
        .find(|f| f.name == "notify" && f.containing_type.as_deref() == Some("MailerService"))
        .expect("class notify method");

    // Param counts.
    assert_eq!(func(&file, "__construct").param_count, 1);
    assert_eq!(notify.param_count, 2);
    assert_eq!(func(&file, "render").param_count, 2);
    assert_eq!(func(&file, "deliver").param_count, 1);

    // Call attribution and receivers: $this->..., Class::, self::, $var->,
    // ?->, new.
    assert_calls(notify, &["find", "warn", "render", "deliver"]);
    let find = notify.calls.iter().find(|c| c.name == "find").unwrap();
    assert_eq!(find.receiver.as_deref(), Some("$this->repo"));
    assert_eq!(find.arg_count, 1);
    let render_call = notify.calls.iter().find(|c| c.name == "render").unwrap();
    assert_eq!(render_call.receiver.as_deref(), Some("$this"));
    assert_eq!(render_call.arg_count, 2);
    let warn = notify.calls.iter().find(|c| c.name == "warn").unwrap();
    assert_eq!(warn.receiver.as_deref(), Some("Log"));
    let deliver_call = notify.calls.iter().find(|c| c.name == "deliver").unwrap();
    assert_eq!(deliver_call.receiver.as_deref(), Some("self"));

    assert_calls(func(&file, "render"), &["strtoupper", "sprintf"]);

    // Static method: plain `new`, qualified `new \A\B\C`, $var-> and ?-> calls.
    let deliver = func(&file, "deliver");
    assert_calls(deliver, &["Transport", "send", "Account", "touch"]);
    let send = deliver.calls.iter().find(|c| c.name == "send").unwrap();
    assert_eq!(send.receiver.as_deref(), Some("$transport"));
    let account_new = deliver.calls.iter().find(|c| c.name == "Account").unwrap();
    assert_eq!(account_new.arg_count, 1);
    let touch = deliver.calls.iter().find(|c| c.name == "touch").unwrap();
    assert_eq!(touch.receiver.as_deref(), Some("$account"));

    // match expression counts as a branch feature.
    assert!(func(&file, "render").features.contains_key("flow:branches"));

    // Imports: plain use binds its last segment, alias binds only the alias,
    // grouped use binds each member against the shared prefix.
    let paths = import_paths(&file);
    for p in [
        "App\\Repositories\\UserRepository",
        "App\\Support\\Logger",
        "App\\Models",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let repo_imp = file
        .imports
        .iter()
        .find(|i| i.path == "App\\Repositories\\UserRepository")
        .unwrap();
    assert!(repo_imp.names.contains(&"UserRepository".to_string()));
    let alias_imp = file
        .imports
        .iter()
        .find(|i| i.path == "App\\Support\\Logger")
        .unwrap();
    assert!(alias_imp.names.contains(&"Log".to_string()));
    assert!(!alias_imp.names.contains(&"Logger".to_string()));
    let group_imp = file
        .imports
        .iter()
        .find(|i| i.path == "App\\Models")
        .unwrap();
    assert!(group_imp.names.contains(&"User".to_string()));
    assert!(group_imp.names.contains(&"Account".to_string()));
}

#[test]
fn extracts_php_helpers() {
    let file = extract_fixture("php", "php/helpers.php");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Free functions plus arrow function / closure assigned to variables.
    for expected in ["slugify", "chunk_lines", "shout", "emit"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // No namespace declared.
    assert_eq!(file.package, None);
    assert_eq!(func(&file, "slugify").containing_type, None);

    // Param counts (closure params resolved through @func.body).
    assert_eq!(func(&file, "slugify").param_count, 1);
    assert_eq!(func(&file, "chunk_lines").param_count, 2);
    assert_eq!(func(&file, "shout").param_count, 1);
    assert_eq!(func(&file, "emit").param_count, 2);

    // Call attribution.
    assert_calls(
        func(&file, "slugify"),
        &["strtolower", "trim", "str_replace"],
    );
    assert_calls(func(&file, "chunk_lines"), &["count"]);
    assert_calls(func(&file, "shout"), &["strtoupper"]);
    assert_calls(func(&file, "emit"), &["trim"]);

    // chunk_lines' foreach/if shape lands in the feature bag.
    let chunk = func(&file, "chunk_lines");
    assert_eq!(chunk.features.get("flow:loops"), Some(&1));
    assert!(chunk.features.contains_key("flow:branches"));
    assert!(chunk.features.contains_key("call:count"));

    // Top-level calls attach to the synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "slugify"));
    assert!(has_call(top, "chunk_lines"));
    assert!(has_call(top, "print_r"));

    // require_once / include with string args become imports.
    let paths = import_paths(&file);
    for p in ["bootstrap.php", "legacy/compat.php"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
}
