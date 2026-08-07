mod common;

use gigagraph::extract::{ExtractedFile, ExtractedFunction};
use common::*;

/// Overload-safe lookup: find by name + param count.
fn func_n<'a>(file: &'a ExtractedFile, name: &str, params: u16) -> &'a ExtractedFunction {
    file.functions
        .iter()
        .find(|f| f.name == name && f.param_count == params)
        .unwrap_or_else(|| {
            let shapes: Vec<String> = file
                .functions
                .iter()
                .map(|f| format!("{}/{}", f.name, f.param_count))
                .collect();
            panic!("function `{name}/{params}` not extracted; got: {shapes:?}")
        })
}

/// Lookup scoped to a containing type (for names reused across types).
fn func_in<'a>(file: &'a ExtractedFile, ty: &str, name: &str) -> &'a ExtractedFunction {
    file.functions
        .iter()
        .find(|f| f.name == name && f.containing_type.as_deref() == Some(ty))
        .unwrap_or_else(|| panic!("function `{ty}.{name}` not extracted"))
}

fn recv_of<'a>(f: &'a ExtractedFunction, callee: &str) -> Option<&'a str> {
    f.calls
        .iter()
        .find(|c| c.name == callee)
        .unwrap_or_else(|| panic!("`{}` has no call to `{callee}`", f.name))
        .receiver
        .as_deref()
}

#[test]
fn extracts_java_functions_and_calls() {
    let file = extract_fixture("java", "java/Catalog.java");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Instance methods, static method, constructor, generic method,
    // inner/static-nested/anonymous class methods, interface default method,
    // record methods.
    for expected in [
        "Catalog",      // constructors
        "addBook",      // overloaded instance method
        "grow",         // private instance method
        "withDefaults", // static method
        "pickLarger",   // generic method
        "summarize",
        "titles",
        "forEachTitle",
        "auditTask",
        "run",          // anonymous class method
        "countFiction", // inner class method
        "averageYear",  // static nested class method
        "describe",     // interface abstract + record method
        "shortLabel",   // interface default method
        "isModern",     // record method
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Package declaration (multi-segment scoped_identifier).
    assert_eq!(file.package.as_deref(), Some("com.example.library"));

    // Overloads: both constructors and both addBook variants extracted.
    assert_eq!(
        file.functions
            .iter()
            .filter(|f| f.name == "Catalog")
            .count(),
        2
    );
    assert_eq!(
        file.functions
            .iter()
            .filter(|f| f.name == "addBook")
            .count(),
        2
    );
    let ctor0 = func_n(&file, "Catalog", 0);
    let ctor1 = func_n(&file, "Catalog", 1);
    assert_eq!(ctor0.containing_type.as_deref(), Some("Catalog"));
    assert_eq!(ctor1.containing_type.as_deref(), Some("Catalog"));

    // containing_type across class / inner class / static nested class /
    // anonymous class / interface / record.
    assert_eq!(
        func(&file, "summarize").containing_type.as_deref(),
        Some("Catalog")
    );
    assert_eq!(
        func(&file, "countFiction").containing_type.as_deref(),
        Some("Shelf")
    );
    assert_eq!(
        func(&file, "averageYear").containing_type.as_deref(),
        Some("Stats")
    );
    // Anonymous classes have no name; nearest named type is the outer class.
    assert_eq!(
        func(&file, "run").containing_type.as_deref(),
        Some("Catalog")
    );
    assert_eq!(
        func(&file, "shortLabel").containing_type.as_deref(),
        Some("Describable")
    );
    assert_eq!(
        func(&file, "isModern").containing_type.as_deref(),
        Some("Book")
    );
    assert!(func_in(&file, "Describable", "describe").calls.is_empty());
    assert!(func_in(&file, "Book", "describe").calls.is_empty());

    // Param counts.
    assert_eq!(func(&file, "grow").param_count, 0);
    assert_eq!(func(&file, "pickLarger").param_count, 2);
    assert_eq!(func(&file, "forEachTitle").param_count, 1);
    assert_eq!(func(&file, "averageYear").param_count, 1);
    let add1 = func_n(&file, "addBook", 1);
    let add2 = func_n(&file, "addBook", 2);

    // Plain call, receiver call, and branch feature in addBook(Book).
    assert_calls(add1, &["size", "grow", "add"]);
    assert_eq!(recv_of(add1, "size"), Some("books"));
    assert_eq!(recv_of(add1, "grow"), None);
    assert!(add1.features.get("flow:branches").is_some());

    // this.m() receiver + object_creation (`new Book(...)`) in addBook(String,int).
    assert_calls(add2, &["addBook", "Book"]);
    assert_eq!(recv_of(add2, "addBook"), Some("this"));
    let new_book = add2.calls.iter().find(|c| c.name == "Book").unwrap();
    assert_eq!(new_book.arg_count, 2);
    assert_eq!(new_book.receiver, None);

    // Statically-imported call: bare `max(...)`.
    let grow = func(&file, "grow");
    assert_calls(grow, &["max"]);
    assert_eq!(grow.calls[0].arg_count, 2);

    // Static factory: `new Catalog()` + receiver call on a local.
    let with_defaults = func(&file, "withDefaults");
    assert_calls(with_defaults, &["Catalog", "addBook"]);
    assert_eq!(recv_of(with_defaults, "addBook"), Some("catalog"));

    // Generic method body call.
    assert_eq!(recv_of(func(&file, "pickLarger"), "compareTo"), Some("a"));

    // summarize: creation via `new StringBuilder()`, chained calls, and
    // System.out.println (field_access receiver).
    let summarize = func(&file, "summarize");
    assert_calls(
        summarize,
        &[
            "StringBuilder",
            "append",
            "describe",
            "toString",
            "trim",
            "println",
        ],
    );
    assert_eq!(
        summarize
            .calls
            .iter()
            .filter(|c| c.name == "append")
            .count(),
        2
    );
    assert_eq!(recv_of(summarize, "toString"), Some("sb"));
    // `sb.toString().trim()`: chained receiver contains parens, so dropped.
    assert_eq!(recv_of(summarize, "trim"), None);
    assert_eq!(recv_of(summarize, "println"), Some("System.out"));
    assert_eq!(summarize.features.get("flow:loops"), Some(&1));
    assert!(summarize.features.contains_key("call:println"));

    // Calls inside a lambda attribute to the enclosing method.
    let titles = func(&file, "titles");
    assert_calls(titles, &["ArrayList", "forEach", "add", "title"]);
    assert_eq!(recv_of(titles, "forEach"), Some("books"));
    assert_eq!(recv_of(titles, "add"), Some("out"));
    assert_eq!(recv_of(titles, "title"), Some("book"));
    let for_each = titles.calls.iter().find(|c| c.name == "forEach").unwrap();
    assert_eq!(for_each.arg_count, 1); // the lambda

    // Anonymous class: `new Runnable()` belongs to auditTask, the call inside
    // run() belongs to run().
    let audit = func(&file, "auditTask");
    assert_calls(audit, &["Runnable"]);
    assert!(!has_call(audit, "summarize"));
    assert_calls(func(&file, "run"), &["summarize"]);

    // Inner + static nested class bodies.
    let count_fiction = func(&file, "countFiction");
    assert_eq!(recv_of(count_fiction, "isModern"), Some("book"));
    assert_eq!(count_fiction.features.get("flow:loops"), Some(&1));
    assert_eq!(count_fiction.features.get("flow:branches"), Some(&1));
    assert_calls(func(&file, "averageYear"), &["year", "isEmpty", "size"]);

    // Interface default method calls its own abstract sibling.
    assert_calls(func(&file, "shortLabel"), &["describe", "substring"]);

    // Field initializer `new ArrayList<>()` runs outside any method: it lands
    // on the synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "ArrayList"));

    // Imports: plain, wildcard, static — paths, bound names, system flags.
    let paths = import_paths(&file);
    for p in [
        "java.util.ArrayList",
        "java.util.List",
        "java.util.function",
        "java.lang.Math.max",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let list_imp = file
        .imports
        .iter()
        .find(|i| i.path == "java.util.List")
        .unwrap();
    assert!(list_imp.names.contains(&"List".to_string()));
    let wildcard = file
        .imports
        .iter()
        .find(|i| i.path == "java.util.function")
        .unwrap();
    assert!(
        wildcard.names.is_empty(),
        "wildcard import binds no names; got {:?}",
        wildcard.names
    );
    let static_imp = file
        .imports
        .iter()
        .find(|i| i.path == "java.lang.Math.max")
        .unwrap();
    assert!(static_imp.names.contains(&"max".to_string()));
    assert!(
        file.imports.iter().all(|i| !i.system),
        "Java imports are never system includes"
    );
}

#[test]
fn extracts_java_records_and_single_segment_package() {
    let file = extract_fixture("java", "java/Probe.java");

    // Single-segment package declaration (bare identifier, not scoped).
    assert_eq!(file.package.as_deref(), Some("tools"));

    // System builtin receiver.
    let describe_runtime = func(&file, "describeRuntime");
    assert_eq!(describe_runtime.containing_type.as_deref(), Some("Probe"));
    assert_eq!(recv_of(describe_runtime, "getProperty"), Some("System"));

    // `new Probe.Sample(...)` — scoped type creation resolves to simple name.
    let make_sample = func(&file, "makeSample");
    let new_sample = make_sample
        .calls
        .iter()
        .find(|c| c.name == "Sample")
        .expect("makeSample should construct Sample");
    assert_eq!(new_sample.arg_count, 2);

    // Record compact constructor + record method. The compact constructor
    // inherits the record header's parameter list.
    let compact = func(&file, "Sample");
    assert_eq!(compact.containing_type.as_deref(), Some("Sample"));
    assert_eq!(compact.param_count, 2);
    assert_calls(compact, &["IllegalArgumentException"]);
    assert!(compact.features.get("flow:branches").is_some());
    let width = func(&file, "width");
    assert_eq!(width.containing_type.as_deref(), Some("Sample"));
    assert_eq!(width.param_count, 0);

    // No imports in this file.
    assert!(file.imports.is_empty());
}
