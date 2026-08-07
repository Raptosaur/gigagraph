mod common;

use common::*;

// NOTE on param counts: the alex-pinkus tree-sitter-swift grammar has no
// parameter-list wrapper node — `parameter` nodes are direct fieldless
// children of `function_declaration` / `init_declaration` — so `@func.params`
// cannot be captured and the extractor reports `param_count == 0` for every
// Swift function. Real param counts are therefore not asserted here.

#[test]
fn extracts_swift_functions() {
    let file = extract_fixture("swift", "swift/library.swift");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Free function, struct method, static method, class method, class-func,
    // enum method, extension method, init.
    for expected in [
        "init",
        "summary",
        "placeholder",
        "add",
        "titles",
        "findBook",
        "makeDefault",
        "label",
        "loadSamples",
        "slugify",
        "catalog",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Methods know their struct/class/enum; extensions attribute to the
    // extended type; free functions have none.
    assert_eq!(func(&file, "init").containing_type.as_deref(), Some("Book"));
    assert_eq!(
        func(&file, "summary").containing_type.as_deref(),
        Some("Book")
    );
    assert_eq!(
        func(&file, "placeholder").containing_type.as_deref(),
        Some("Book")
    );
    assert_eq!(
        func(&file, "add").containing_type.as_deref(),
        Some("Library")
    );
    assert_eq!(
        func(&file, "makeDefault").containing_type.as_deref(),
        Some("Library")
    );
    assert_eq!(
        func(&file, "label").containing_type.as_deref(),
        Some("Genre")
    );
    // Extension method: containing_type comes from `extension Library`.
    assert_eq!(
        func(&file, "loadSamples").containing_type.as_deref(),
        Some("Library")
    );
    assert_eq!(func(&file, "slugify").containing_type, None);
    assert_eq!(func(&file, "catalog").containing_type, None);

    // Plain receiver call.
    let add_fn = func(&file, "add");
    let append = add_fn.calls.iter().find(|c| c.name == "append").unwrap();
    assert_eq!(append.receiver.as_deref(), Some("books"));
    assert_eq!(append.arg_count, 1);

    // `Book(...)` initializer call: callee name is the type.
    let placeholder_fn = func(&file, "placeholder");
    let book_call = placeholder_fn
        .calls
        .iter()
        .find(|c| c.name == "Book")
        .unwrap();
    assert_eq!(book_call.receiver, None);
    assert_eq!(book_call.arg_count, 2);

    // Trailing-closure call still captures the callee name + receiver.
    let titles_fn = func(&file, "titles");
    let map_call = titles_fn.calls.iter().find(|c| c.name == "map").unwrap();
    assert_eq!(map_call.receiver.as_deref(), Some("books"));

    // class func body: bare constructor call + receiver calls.
    let make_default = func(&file, "makeDefault");
    assert_calls(make_default, &["Library", "add", "placeholder"]);
    let lib_add = make_default.calls.iter().find(|c| c.name == "add").unwrap();
    assert_eq!(lib_add.receiver.as_deref(), Some("library"));
    let static_call = make_default
        .calls
        .iter()
        .find(|c| c.name == "placeholder")
        .unwrap();
    assert_eq!(static_call.receiver.as_deref(), Some("Book"));

    // Calls inside a closure attribute to the enclosing function; `self`
    // receiver survives.
    let load_samples = func(&file, "loadSamples");
    assert_calls(load_samples, &["forEach", "add", "Book"]);
    let self_add = load_samples.calls.iter().find(|c| c.name == "add").unwrap();
    assert_eq!(self_add.receiver.as_deref(), Some("self"));
    let closure_book = load_samples
        .calls
        .iter()
        .find(|c| c.name == "Book")
        .unwrap();
    assert_eq!(closure_book.arg_count, 2);

    // Chained call: `raw.lowercased().replacingOccurrences(...)` — both
    // callees captured; the chained receiver (contains parens) is dropped.
    let slugify_fn = func(&file, "slugify");
    assert_calls(slugify_fn, &["lowercased", "replacingOccurrences"]);
    let chained = slugify_fn
        .calls
        .iter()
        .find(|c| c.name == "replacingOccurrences")
        .unwrap();
    assert_eq!(chained.receiver, None);
    assert_eq!(chained.arg_count, 2);
    let lowercased = slugify_fn
        .calls
        .iter()
        .find(|c| c.name == "lowercased")
        .unwrap();
    assert_eq!(lowercased.receiver.as_deref(), Some("raw"));

    // Closure body call inside a chained trailing-closure call attributes to
    // the enclosing free function.
    let catalog_fn = func(&file, "catalog");
    assert_calls(catalog_fn, &["titles", "map", "slugify", "joined"]);
    let joined = catalog_fn
        .calls
        .iter()
        .find(|c| c.name == "joined")
        .unwrap();
    assert_eq!(joined.receiver.as_deref(), Some("lines"));
    assert_eq!(joined.arg_count, 1);

    // Control flow lands in the feature bag.
    let find_book = func(&file, "findBook");
    assert_eq!(find_book.features.get("flow:loops"), Some(&1));
    assert_eq!(find_book.features.get("flow:branches"), Some(&1));
    assert_eq!(catalog_fn.features.get("flow:loops"), Some(&1)); // while
    assert_eq!(func(&file, "label").features.get("flow:branches"), Some(&1)); // switch
    assert_eq!(slugify_fn.features.get("flow:branches"), Some(&1)); // guard

    // Top-level statements become the synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "makeDefault"));
    assert!(has_call(top, "print"));
    assert!(has_call(top, "catalog"));
    let top_make = top.calls.iter().find(|c| c.name == "makeDefault").unwrap();
    assert_eq!(top_make.receiver.as_deref(), Some("Library"));

    // Imports: plain module and dotted submodule; last component is the
    // bound name; Swift has no system-include distinction.
    let paths = import_paths(&file);
    for p in ["Foundation", "UIKit.UIView"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let foundation = file
        .imports
        .iter()
        .find(|i| i.path == "Foundation")
        .unwrap();
    assert!(foundation.names.contains(&"Foundation".to_string()));
    assert!(!foundation.system);
    let submodule = file
        .imports
        .iter()
        .find(|i| i.path == "UIKit.UIView")
        .unwrap();
    assert_eq!(submodule.names, vec!["UIView".to_string()]);
    assert!(!submodule.system);

    // Swift has no package declaration.
    assert_eq!(file.package, None);
}

#[test]
fn extracts_swift_shapes() {
    let file = extract_fixture("swift", "swift/shapes.swift");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Protocol requirement, class init, methods, nested function.
    for expected in [
        "describe", "init", "lookup", "warm", "store", "report", "heading", "encode",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    assert_eq!(
        func(&file, "describe").containing_type.as_deref(),
        Some("Describable")
    );
    assert_eq!(
        func(&file, "init").containing_type.as_deref(),
        Some("Cache")
    );
    assert_eq!(
        func(&file, "warm").containing_type.as_deref(),
        Some("Cache")
    );
    // Nested function: no enclosing type.
    assert_eq!(func(&file, "heading").containing_type, None);

    let init_fn = func(&file, "init");
    let reserve = init_fn
        .calls
        .iter()
        .find(|c| c.name == "reserveCapacity")
        .unwrap();
    assert_eq!(reserve.receiver.as_deref(), Some("storage"));
    assert_eq!(reserve.arg_count, 1);

    // Subscript access (`storage[key]`, `keys[index]`) must NOT extract as a
    // call.
    assert!(!has_call(func(&file, "lookup"), "storage"));
    assert!(!has_call(func(&file, "store"), "storage"));
    assert!(!has_call(func(&file, "warm"), "keys"));

    // repeat-while is a loop; bare method call captured with args.
    let warm_fn = func(&file, "warm");
    assert_eq!(warm_fn.features.get("flow:loops"), Some(&1));
    let store_call = warm_fn.calls.iter().find(|c| c.name == "store").unwrap();
    assert_eq!(store_call.receiver, None);
    assert_eq!(store_call.arg_count, 2);

    // if-let is a branch.
    assert_eq!(
        func(&file, "lookup").features.get("flow:branches"),
        Some(&1)
    );

    // Nested-function attribution: the inner body's call belongs to the
    // inner function, the call TO the inner function belongs to the outer.
    let heading_fn = func(&file, "heading");
    assert_calls(heading_fn, &["uppercased"]);
    let report_fn = func(&file, "report");
    assert_calls(report_fn, &["heading", "encode", "String"]);
    assert!(!has_call(report_fn, "uppercased"));
    // do/catch counts as a branch.
    assert!(report_fn.features.get("flow:branches").is_some());

    // Ternary counts as a branch; receiver call inside it captured.
    let encode_fn = func(&file, "encode");
    assert_eq!(encode_fn.features.get("flow:branches"), Some(&1));
    let lookup_call = encode_fn.calls.iter().find(|c| c.name == "lookup").unwrap();
    assert_eq!(lookup_call.receiver.as_deref(), Some("cache"));

    // Import binds the module name.
    let combine = file.imports.iter().find(|i| i.path == "Combine").unwrap();
    assert_eq!(combine.names, vec!["Combine".to_string()]);
    assert_eq!(file.package, None);
}
