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
fn captures_typescript_di_types() {
    let file = extract_fixture("ts", "typescript/di.ts");

    // Declared fields: plain, nested (cdk.Bucket keeps last segment), and
    // abstract on an abstract class.
    assert_eq!(
        field_of(&file, "EmailNotifier", "transport"),
        Some("SmtpTransport")
    );
    assert_eq!(field_of(&file, "EmailNotifier", "bucket"), Some("Bucket"));
    assert_eq!(field_of(&file, "BaseHandler", "queue"), Some("JobQueue"));

    // Constructor parameter properties: accessibility modifier, bare
    // readonly, and optional (`?:`) forms all become fields.
    assert_eq!(field_of(&file, "EmailNotifier", "mailer"), Some("Mailer"));
    assert_eq!(field_of(&file, "EmailNotifier", "clock"), Some("Clock"));
    assert_eq!(field_of(&file, "EmailNotifier", "tag"), Some("TagService"));
    assert_eq!(
        field_of(&file, "EmailNotifier", "retries"),
        Some("RetryPolicy")
    );
    // `this.fallback = new ConsoleNotifier()` joins the untyped (`any`)
    // declaration to the constructed type.
    assert_eq!(
        field_of(&file, "EmailNotifier", "fallback"),
        Some("ConsoleNotifier")
    );
    // A plain (non-property) ctor param is a local, not a field.
    assert_eq!(field_of(&file, "EmailNotifier", "plainLimit"), None);
    assert_eq!(
        local_of(func(&file, "constructor"), "plainLimit"),
        Some("RateLimit")
    );

    // Locals in a method body: constructed, annotated, and nested-annotated.
    let notify = func(&file, "notify");
    assert_eq!(local_of(notify, "svc"), Some("AuthService"));
    assert_eq!(local_of(notify, "helper"), Some("Helper"));
    assert_eq!(local_of(notify, "stack"), Some("Stack"));

    // Hierarchy: extends, implements, abstract-class extends,
    // interface-extends-interface, and extends of a member expression.
    for edge in [
        ("EmailNotifier", "BaseHandler"),
        ("EmailNotifier", "Notifier"),
        ("AuditedNotifier", "Notifier"),
        ("InfraStack", "Stack"),
    ] {
        let edge = (edge.0.to_string(), edge.1.to_string());
        assert!(
            file.hierarchy.contains(&edge),
            "missing hierarchy edge {edge:?}; got {:?}",
            file.hierarchy
        );
    }
}

#[test]
fn captures_javascript_di_types() {
    let file = extract_fixture("js", "javascript/store.js");

    // `this.db = new Database()` in the ctor: owner is the enclosing class.
    assert_eq!(field_of(&file, "Store", "db"), Some("Database"));

    // `const q = new QueryBuilder()` attaches to the enclosing method.
    assert_eq!(local_of(func(&file, "run"), "q"), Some("QueryBuilder"));

    // extends with a plain identifier and with a member expression.
    for edge in [("Store", "BaseStore"), ("RemoteStore", "HttpStore")] {
        let edge = (edge.0.to_string(), edge.1.to_string());
        assert!(
            file.hierarchy.contains(&edge),
            "missing hierarchy edge {edge:?}; got {:?}",
            file.hierarchy
        );
    }
}

#[test]
fn captures_new_expression_arguments() {
    use gigagraph::extract::LitKind;

    let src = r#"
import * as lambda from 'aws-cdk-lib/aws-lambda';
const fn = new lambda.Function(this, 'Fn', {
  handler: 'index.handler',
  code: lambda.Code.fromAsset('assets'),
});
const bare = new Widget('left', 2);
"#;
    let file = extract_str("ts", src);
    let top = func(&file, "(toplevel)");

    // Member-form `new ns.Ctor(...)`: name from the property, receiver from
    // the object, arguments harvested.
    let ctor = top
        .calls
        .iter()
        .find(|c| c.name == "Function")
        .expect("member-form new_expression not captured");
    assert_eq!(ctor.receiver.as_deref(), Some("lambda"));
    assert_eq!(ctor.arg_count, 3, "this + id string + props object");
    // Object props arrive as Ident-key followed by Str-value at the same
    // argument index (the window shape endpoints.rs pairs back up).
    let lits: Vec<(LitKind, &str)> = ctor
        .arg_lits
        .iter()
        .map(|l| (l.kind, l.text.as_str()))
        .collect();
    assert!(
        lits.contains(&(LitKind::Str, "Fn")),
        "construct id missing; got {lits:?}"
    );
    let key_pos = lits
        .iter()
        .position(|l| *l == (LitKind::Ident, "handler"))
        .unwrap_or_else(|| panic!("handler key ident missing; got {lits:?}"));
    assert_eq!(
        lits.get(key_pos + 1),
        Some(&(LitKind::Str, "index.handler")),
        "handler value must follow its key; got {lits:?}"
    );

    // The nested Code.fromAsset call still surfaces as its own call.
    let asset = top
        .calls
        .iter()
        .find(|c| c.name == "fromAsset")
        .expect("nested fromAsset call missing");
    assert_eq!(asset.receiver.as_deref(), Some("lambda.Code"));

    // Identifier-form constructor: the name-only and with-arguments patterns
    // both match — the extractor must merge them into ONE call, with args.
    let widgets: Vec<_> = top.calls.iter().filter(|c| c.name == "Widget").collect();
    assert_eq!(widgets.len(), 1, "overlapping new patterns must dedup");
    assert_eq!(widgets[0].arg_count, 2);
    assert!(
        widgets[0]
            .arg_lits
            .iter()
            .any(|l| l.kind == LitKind::Str && l.text == "left"),
        "bare-new arguments not harvested: {:?}",
        widgets[0].arg_lits
    );
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
