mod common;

use common::*;

#[test]
fn extracts_go_functions_and_methods() {
    let file = extract_fixture("go", "go/server.go");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Plain function + pointer-receiver methods.
    for expected in ["NewServer", "Run", "handle", "Close", "Shutdown"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Go files declare their package; it feeds qualified names.
    assert_eq!(file.package.as_deref(), Some("mypkg"));

    // Known limitation: a method's receiver type is a sibling field of the
    // declaration, not an ancestor node, so TYPE_KINDS can't resolve it.
    assert_eq!(func(&file, "Run").containing_type, None);
    assert_eq!(func(&file, "NewServer").containing_type, None);

    // Param counts (receiver is not a parameter).
    assert_eq!(func(&file, "NewServer").param_count, 1);
    assert_eq!(func(&file, "Run").param_count, 0);
    assert_eq!(func(&file, "handle").param_count, 1);
}

#[test]
fn attributes_go_calls_with_receivers() {
    let file = extract_fixture("go", "go/server.go");

    assert_calls(func(&file, "NewServer"), &["Printf"]);
    let printf = func(&file, "NewServer")
        .calls
        .iter()
        .find(|c| c.name == "Printf")
        .unwrap();
    assert_eq!(printf.receiver.as_deref(), Some("stdlog"));

    // Run: pkg call, deferred method, goroutine method, and a call inside a
    // `go func() { ... }()` closure — all attributed to Run itself.
    assert_calls(
        func(&file, "Run"),
        &["Listen", "Errorf", "Close", "Println", "Accept", "handle"],
    );
    let handle_call = func(&file, "Run")
        .calls
        .iter()
        .find(|c| c.name == "handle")
        .unwrap();
    assert_eq!(handle_call.receiver.as_deref(), Some("s"));
    assert_eq!(handle_call.arg_count, 1);

    // handle: defer, chained-selector receiver, cross-file helpers.
    assert_calls(
        func(&file, "handle"),
        &[
            "Close",
            "Lock",
            "Unlock",
            "ReadLine",
            "Classify",
            "Fprintln",
            "Normalize",
        ],
    );
    let lock = func(&file, "handle")
        .calls
        .iter()
        .find(|c| c.name == "Lock")
        .unwrap();
    assert_eq!(lock.receiver.as_deref(), Some("s.mu"));

    // Method value invoked through a variable.
    assert_calls(func(&file, "Shutdown"), &["closer"]);
}

#[test]
fn captures_go_imports() {
    let file = extract_fixture("go", "go/server.go");
    let paths = import_paths(&file);
    for p in ["fmt", "net", "sync", "log", "net/http/pprof"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }

    // Aliased import binds its local name.
    let log_imp = file.imports.iter().find(|i| i.path == "log").unwrap();
    assert!(log_imp.names.contains(&"stdlog".to_string()));

    // Blank import: path only, no bound name.
    let blank = file
        .imports
        .iter()
        .find(|i| i.path == "net/http/pprof")
        .unwrap();
    assert!(blank.names.is_empty());
}

#[test]
fn extracts_go_helpers_and_control_flow() {
    let file = extract_fixture("go", "go/util.go");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();
    for expected in ["ReadLine", "Normalize", "Classify"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(file.package.as_deref(), Some("mypkg"));
    assert_eq!(func(&file, "ReadLine").param_count, 1);

    assert_calls(
        func(&file, "ReadLine"),
        &["NewReader", "ReadString", "TrimSpace"],
    );
    // Nested call: strings.ToLower(punct.Replace(s)) yields both.
    assert_calls(func(&file, "Normalize"), &["ToLower", "Replace"]);

    // Classify: switch + for + if land in the feature bag.
    let classify = func(&file, "Classify");
    assert_calls(classify, &["Normalize", "len"]);
    assert_eq!(classify.features.get("flow:loops"), Some(&1));
    assert_eq!(classify.features.get("flow:branches"), Some(&2));

    // Package-level `var punct = strings.NewReplacer(...)` becomes a
    // toplevel call.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "NewReplacer"));
    let repl = top.calls.iter().find(|c| c.name == "NewReplacer").unwrap();
    assert_eq!(repl.receiver.as_deref(), Some("strings"));
}

#[test]
fn captures_go_struct_field_types() {
    let file = extract_fixture("go", "go/wiring.go");

    // Owner comes from the explicit @field.owner capture (the type_spec name
    // is a sibling of the struct body, not an ancestor — the ancestor scan
    // would find nothing).
    assert_eq!(field_of(&file, "Dispatcher", "store"), Some("OrderStore"));
    assert_eq!(field_of(&file, "Dispatcher", "name"), Some("string"));

    // Pointer-wrapped and pointer-to-qualified field types keep the simple
    // type name (*sql.DB -> DB, *stdlog.Logger -> Logger).
    assert_eq!(field_of(&file, "SQLOrderStore", "db"), Some("DB"));
    assert_eq!(field_of(&file, "SQLOrderStore", "logger"), Some("Logger"));

    // Fields never leak onto the wrong owner.
    assert_eq!(field_of(&file, "SQLOrderStore", "store"), None);
}

#[test]
fn captures_go_typed_locals() {
    let file = extract_fixture("go", "go/wiring.go");

    // Interface-typed parameter: this is the binding that lets `store.Save()`
    // resolve through the locals rung today.
    let process = func(&file, "Process");
    assert_eq!(local_of(process, "store"), Some("OrderStore"));
    assert_eq!(local_of(process, "count"), Some("int"));
    let save = process.calls.iter().find(|c| c.name == "Save").unwrap();
    assert_eq!(save.receiver.as_deref(), Some("store"));

    // `var x T` and `var y *T` inside a body.
    let rewire = func(&file, "Rewire");
    assert_eq!(local_of(rewire, "backup"), Some("OrderStore"));
    assert_eq!(local_of(rewire, "d"), Some("Dispatcher"));

    // Method receivers are parameter_declarations too, so the receiver name
    // binds to its (pointer-stripped) type inside the method.
    let flush = func(&file, "Flush");
    assert_eq!(local_of(flush, "d"), Some("Dispatcher"));

    // Known gap: `s.store.Save()` receiver text is dotted, which the
    // resolver's bare branch rejects — but the call is still captured.
    let dotted = flush.calls.iter().find(|c| c.name == "Save").unwrap();
    assert_eq!(dotted.receiver.as_deref(), Some("d.store"));

    // Go is structural: no inheritance clause, so no hierarchy edges.
    assert!(file.hierarchy.is_empty());
}
