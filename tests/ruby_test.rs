mod common;

use common::*;

#[test]
fn extracts_ruby_service() {
    let file = extract_fixture("rb", "ruby/service.rb");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Instance methods (positional/keyword/default params, paren-full) and a
    // `def self.` singleton method.
    for expected in [
        "initialize",
        "from_json",
        "summarize",
        "render_row",
        "build_label",
        "total_for",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Methods know their class — including the singleton method.
    assert_eq!(
        func(&file, "summarize").containing_type.as_deref(),
        Some("ReportService")
    );
    assert_eq!(
        func(&file, "from_json").containing_type.as_deref(),
        Some("ReportService")
    );

    // Param counts: positional + keyword, singleton, optional default.
    assert_eq!(func(&file, "initialize").param_count, 2);
    assert_eq!(func(&file, "from_json").param_count, 1);
    assert_eq!(func(&file, "summarize").param_count, 1);
    assert_eq!(func(&file, "total_for").param_count, 1);

    // Call attribution: receiver call, paren-less `puts`, the `.each` block
    // call itself, and calls inside the block body land in the enclosing
    // method.
    assert_calls(
        func(&file, "summarize"),
        &["take", "each", "puts", "render_row", "total_for"],
    );
    assert_calls(func(&file, "from_json"), &["new", "parse"]);
    let parse_call = func(&file, "from_json")
        .calls
        .iter()
        .find(|c| c.name == "parse")
        .unwrap();
    assert_eq!(parse_call.receiver.as_deref(), Some("JSON"));

    // `self.build_label(row)` keeps its `self` receiver and arg count.
    let build_label_call = func(&file, "render_row")
        .calls
        .iter()
        .find(|c| c.name == "build_label")
        .unwrap();
    assert_eq!(build_label_call.receiver.as_deref(), Some("self"));
    assert_eq!(build_label_call.arg_count, 1);

    // total_for's `while` lands in the feature bag; render_row's ternary is a
    // branch.
    let total_fn = func(&file, "total_for");
    assert_eq!(total_fn.features.get("flow:loops"), Some(&1));
    assert!(total_fn.features.contains_key("call:fetch"));
    assert!(
        func(&file, "render_row")
            .features
            .contains_key("flow:branches")
    );

    // Top-level `ReportService.from_json(...)` / `service.summarize(5)` become
    // the synthetic toplevel function's calls, receivers intact.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "from_json"));
    assert!(has_call(top, "summarize"));
    let top_from_json = top.calls.iter().find(|c| c.name == "from_json").unwrap();
    assert_eq!(top_from_json.receiver.as_deref(), Some("ReportService"));

    // Imports: `require` and `require_relative`, distinguished via the bound
    // method name.
    let paths = import_paths(&file);
    for p in ["json", "helper"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let json_imp = file.imports.iter().find(|i| i.path == "json").unwrap();
    assert!(json_imp.names.contains(&"require".to_string()));
    let rel_imp = file.imports.iter().find(|i| i.path == "helper").unwrap();
    assert!(rel_imp.names.contains(&"require_relative".to_string()));
}

#[test]
fn extracts_ruby_type_information() {
    let file = extract_fixture("rb", "ruby/di.rb");

    // `@store = DbStore.new` — the field key keeps the `@` sigil, matching
    // the receiver text of later `@store.persist` calls.
    assert_eq!(field_of(&file, "Service", "@store"), Some("DbStore"));

    // `x = DbStore.new` becomes a typed local of the enclosing method.
    assert_eq!(local_of(func(&file, "setup"), "x"), Some("DbStore"));

    // `@store = store` in initialize is a plain param assignment — no
    // construction, no annotation, so no field capture from it.
    assert!(
        func(&file, "initialize").locals.is_empty(),
        "initialize should bind no typed locals"
    );

    // `class Service < Base` yields the inheritance edge.
    assert!(file.hierarchy.contains(&("Service".into(), "Base".into())));
}

#[test]
fn extracts_ruby_helper_module() {
    let file = extract_fixture("rb", "ruby/helper.rb");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    for expected in ["label", "pad", "divider"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Modules count as containing types too.
    assert_eq!(
        func(&file, "label").containing_type.as_deref(),
        Some("Format")
    );

    // Default param counts; paren-less `def divider` still resolves to zero.
    assert_eq!(func(&file, "pad").param_count, 2);
    assert_eq!(func(&file, "divider").param_count, 0);

    // Self-call plus chained receiver calls.
    assert_calls(func(&file, "pad"), &["label", "ljust"]);
    assert_calls(func(&file, "label"), &["to_s", "strip", "capitalize"]);
}
