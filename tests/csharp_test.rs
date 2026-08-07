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
fn extracts_csharp_functions_and_calls() {
    let file = extract_fixture("cs", "csharp/Catalog.cs");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Constructors, expression-bodied method, static method, overloaded
    // instance methods, async method, local function, generic method,
    // interface method, record methods.
    for expected in [
        "Catalog",      // constructors
        "Count",        // expression-bodied method
        "WithDefaults", // static factory
        "AddBook",      // overloaded instance method
        "Grow",         // private instance method
        "Describe",     // interface abstract + class method
        "LoadAsync",    // async method
        "CountLines",   // local function inside LoadAsync
        "ModernTitles",
        "First",    // generic method
        "Label",    // record expression-bodied method
        "IsModern", // record method
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Block namespace declaration feeds the package field.
    assert_eq!(file.package.as_deref(), Some("Acme.Store"));

    // Overloads: both constructors and both AddBook variants extracted.
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
            .filter(|f| f.name == "AddBook")
            .count(),
        2
    );
    let ctor0 = func_n(&file, "Catalog", 0);
    let ctor1 = func_n(&file, "Catalog", 1);
    assert_eq!(ctor0.containing_type.as_deref(), Some("Catalog"));
    assert_eq!(ctor1.containing_type.as_deref(), Some("Catalog"));

    // containing_type across class / interface / record / nested local fn.
    assert_eq!(
        func(&file, "Count").containing_type.as_deref(),
        Some("Catalog")
    );
    assert_eq!(
        func(&file, "Label").containing_type.as_deref(),
        Some("Book")
    );
    assert_eq!(
        func(&file, "IsModern").containing_type.as_deref(),
        Some("Book")
    );
    // Local functions bind to the nearest enclosing type.
    assert_eq!(
        func(&file, "CountLines").containing_type.as_deref(),
        Some("Catalog")
    );
    assert!(func_in(&file, "IDescribable", "Describe").calls.is_empty());

    // Param counts.
    assert_eq!(func(&file, "Count").param_count, 0);
    assert_eq!(func(&file, "LoadAsync").param_count, 1);
    assert_eq!(func(&file, "CountLines").param_count, 1);
    assert_eq!(func(&file, "First").param_count, 1);

    // Constructor body: Console builtin receiver call.
    assert_calls(ctor1, &["WriteLine"]);
    assert_eq!(recv_of(ctor1, "WriteLine"), Some("Console"));

    // Expression-bodied `Count() => books.Count` has a property access, not a
    // call.
    assert!(func(&file, "Count").calls.is_empty());

    // Static factory: `new Catalog(8)` + receiver call on a local + nested
    // `new Book(...)` inside the argument list.
    let with_defaults = func(&file, "WithDefaults");
    assert_calls(with_defaults, &["Catalog", "AddBook", "Book"]);
    assert_eq!(recv_of(with_defaults, "AddBook"), Some("catalog"));
    let new_book = with_defaults
        .calls
        .iter()
        .find(|c| c.name == "Book")
        .unwrap();
    assert_eq!(new_book.arg_count, 2);
    assert_eq!(new_book.receiver, None);

    // Plain call, receiver call, and branch feature in AddBook(Book).
    let add1 = func_n(&file, "AddBook", 1);
    assert_calls(add1, &["Grow", "Add"]);
    assert_eq!(recv_of(add1, "Grow"), None);
    assert_eq!(recv_of(add1, "Add"), Some("books"));
    assert!(add1.features.get("flow:branches").is_some());

    // this.M() receiver + object creation in AddBook(string, int).
    let add2 = func_n(&file, "AddBook", 2);
    assert_calls(add2, &["AddBook", "Book"]);
    assert_eq!(recv_of(add2, "AddBook"), Some("this"));

    // `using static System.Math;` makes `Max(...)` a bare call.
    let grow = func(&file, "Grow");
    assert_calls(grow, &["Max"]);
    assert_eq!(grow.calls[0].arg_count, 2);

    // Describe: creation, receiver calls, chained call, builtin receiver,
    // foreach loop feature.
    let describe = func_in(&file, "Catalog", "Describe");
    assert_calls(
        describe,
        &[
            "StringBuilder",
            "Append",
            "Label",
            "ToString",
            "Trim",
            "WriteLine",
        ],
    );
    assert_eq!(recv_of(describe, "Append"), Some("sb"));
    assert_eq!(recv_of(describe, "Label"), Some("book"));
    assert_eq!(recv_of(describe, "ToString"), Some("sb"));
    // `sb.ToString().Trim()`: chained receiver contains parens, so dropped.
    assert_eq!(recv_of(describe, "Trim"), None);
    assert_eq!(recv_of(describe, "WriteLine"), Some("Console"));
    assert_eq!(describe.features.get("flow:loops"), Some(&1));
    assert!(describe.features.contains_key("call:WriteLine"));

    // Async method: await'd aliased-receiver call; the local function's body
    // belongs to the local function, not the enclosing method.
    let load_async = func(&file, "LoadAsync");
    assert_calls(load_async, &["ReadAllTextAsync", "CountLines"]);
    assert_eq!(recv_of(load_async, "ReadAllTextAsync"), Some("F"));
    assert!(!has_call(load_async, "Split"));
    let count_lines = func(&file, "CountLines");
    assert_calls(count_lines, &["Split"]);
    assert_eq!(recv_of(count_lines, "Split"), Some("s"));

    // Generic creation (`new List<string>()`), generic member call with a
    // `this` receiver, ternary branch, for-loop feature.
    let modern = func(&file, "ModernTitles");
    assert_calls(
        modern,
        &["List", "Add", "IsModern", "Label", "First", "WriteLine"],
    );
    assert_eq!(recv_of(modern, "Add"), Some("titles"));
    assert_eq!(recv_of(modern, "IsModern"), Some("book"));
    assert_eq!(recv_of(modern, "First"), Some("this"));
    assert_eq!(modern.features.get("flow:loops"), Some(&1));
    assert_eq!(modern.features.get("flow:branches"), Some(&1));

    // `throw new InvalidOperationException(...)` is a constructor call.
    let first = func(&file, "First");
    assert_calls(first, &["InvalidOperationException"]);
    assert!(first.features.get("flow:branches").is_some());

    // Field initializer `new List<Book>()` runs outside any method: it lands
    // on the synthetic toplevel function.
    let top = func(&file, "(toplevel)");
    assert!(has_call(top, "List"));

    // Imports: plain (single- and multi-segment), alias, static.
    let paths = import_paths(&file);
    for p in [
        "System",
        "System.Collections.Generic",
        "System.Text",
        "System.Threading.Tasks",
        "System.IO.File",
        "System.Math",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    let alias_imp = file
        .imports
        .iter()
        .find(|i| i.path == "System.IO.File")
        .unwrap();
    assert!(alias_imp.names.contains(&"F".to_string()));
    let plain_imp = file
        .imports
        .iter()
        .find(|i| i.path == "System.Collections.Generic")
        .unwrap();
    assert!(
        plain_imp.names.is_empty(),
        "namespace using binds no names; got {:?}",
        plain_imp.names
    );
    let static_imp = file
        .imports
        .iter()
        .find(|i| i.path == "System.Math")
        .unwrap();
    assert!(
        static_imp.names.is_empty(),
        "static using binds no names; got {:?}",
        static_imp.names
    );
    assert!(
        file.imports.iter().all(|i| !i.system),
        "C# usings are never system includes"
    );
}

#[test]
fn extracts_csharp_file_scoped_namespace_and_structs() {
    let file = extract_fixture("cs", "csharp/Validators.cs");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    for expected in [
        "IsValidTitle",
        "MaxLength",
        "BuildSample",
        "Classify",
        "SafeLabel",
        "Width",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // File-scoped namespace declaration feeds the package field.
    assert_eq!(file.package.as_deref(), Some("Acme.Store"));

    // Static class and struct containing types.
    assert_eq!(
        func(&file, "IsValidTitle").containing_type.as_deref(),
        Some("Validators")
    );
    assert_eq!(
        func(&file, "Width").containing_type.as_deref(),
        Some("Slot")
    );
    assert_eq!(func(&file, "Width").param_count, 0);

    // Predefined-type receiver (`string.IsNullOrWhiteSpace`), local receiver,
    // bare sibling call, if-branch feature.
    let is_valid = func(&file, "IsValidTitle");
    assert_calls(is_valid, &["IsNullOrWhiteSpace", "Trim", "MaxLength"]);
    assert_eq!(recv_of(is_valid, "IsNullOrWhiteSpace"), Some("string"));
    assert_eq!(recv_of(is_valid, "Trim"), Some("title"));
    assert_eq!(recv_of(is_valid, "MaxLength"), None);
    assert_eq!(is_valid.features.get("flow:branches"), Some(&1));

    // Builtin receivers Convert and Math in an expression-bodied static
    // method.
    let max_length = func(&file, "MaxLength");
    assert_calls(max_length, &["ToInt32", "Pow"]);
    assert_eq!(recv_of(max_length, "ToInt32"), Some("Convert"));
    assert_eq!(recv_of(max_length, "Pow"), Some("Math"));
    let pow = max_length.calls.iter().find(|c| c.name == "Pow").unwrap();
    assert_eq!(pow.arg_count, 2);

    // Cross-file calls into Catalog.cs: constructor + receiver methods.
    let build = func(&file, "BuildSample");
    assert_calls(build, &["Catalog", "AddBook", "Count"]);
    let new_catalog = build.calls.iter().find(|c| c.name == "Catalog").unwrap();
    assert_eq!(new_catalog.arg_count, 1);
    assert_eq!(recv_of(build, "AddBook"), Some("catalog"));
    assert_eq!(recv_of(build, "Count"), Some("catalog"));
    assert_eq!(
        build.calls.iter().filter(|c| c.name == "AddBook").count(),
        2
    );
    assert_eq!(build.features.get("flow:loops"), Some(&1));

    // switch expression + nested ternary both count as branches.
    assert_eq!(
        func(&file, "Classify").features.get("flow:branches"),
        Some(&2)
    );

    // Conditional-access call `book?.Label()` keeps its receiver.
    let safe_label = func(&file, "SafeLabel");
    assert_calls(safe_label, &["Label"]);
    assert_eq!(recv_of(safe_label, "Label"), Some("book"));

    // Imports: global using + plain using.
    let paths = import_paths(&file);
    for p in ["System.Linq", "System"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    assert!(file.imports.iter().all(|i| !i.system));
}

#[test]
fn extracts_csharp_type_information() {
    let file = extract_fixture("cs", "csharp/Wiring.cs");

    // Declared field: `private readonly IUserStore _store;`. This capture is
    // what lets the resolver narrow the bare-field receiver `_store.Save()`
    // (locals rung misses, fields rung hits).
    assert_eq!(field_of(&file, "UserService", "_store"), Some("IUserStore"));

    // Auto-property `public IUserStore Store { get; }`.
    assert_eq!(field_of(&file, "UserService", "Store"), Some("IUserStore"));

    // Generic field keeps the simple outer name (`List<string>` -> `List`).
    assert_eq!(field_of(&file, "DbUserStore", "_rows"), Some("List"));

    // Primary-constructor params become fields of the declaring type
    // (class and record positional forms).
    assert_eq!(field_of(&file, "AuditService", "store"), Some("IUserStore"));
    assert_eq!(field_of(&file, "AuditService", "_audit"), Some("IUserStore"));
    assert_eq!(field_of(&file, "Wiring", "Store"), Some("IUserStore"));

    // Predefined types (string/int) are not DI-relevant and are skipped.
    assert_eq!(field_of(&file, "Wiring", "Label"), None);
    assert_eq!(field_of(&file, "Slot", "Rank"), None);

    // Hierarchy: class implements, interface extends, qualified base keeps
    // its last segment, generic base keeps the simple name, record base.
    for (t, b) in [
        ("DbUserStore", "IUserStore"),
        ("IAuditStore", "IUserStore"),
        ("MemoryStore", "IUserStore"),
        ("AuditService", "IAuditSource"),
        ("Wiring", "IAuditSource"),
        ("Slot", "IComparable"),
    ] {
        assert!(
            file.hierarchy.contains(&(t.into(), b.into())),
            "missing hierarchy edge {t} -> {b}; got {:?}",
            file.hierarchy
        );
    }

    // Interface-typed ctor param is a local of the constructor.
    let ctor = func(&file, "UserService");
    assert_eq!(local_of(ctor, "store"), Some("IUserStore"));

    // Method param with a user type; predefined-type params are skipped.
    let compare = func(&file, "CompareTo");
    assert_eq!(local_of(compare, "other"), Some("Slot"));
    let register = func(&file, "Register");
    assert_eq!(local_of(register, "name"), None);

    // Locals in Wire(): `var x = new T()` recovers T from the initializer,
    // `T x = new T()` and `IUserStore x = new MemoryStore()` from the
    // declared type (declared type wins for the interface-typed binding).
    let wire = func(&file, "Wire");
    assert_eq!(local_of(wire, "store"), Some("DbUserStore"));
    assert_eq!(local_of(wire, "backup"), Some("DbUserStore"));
    assert_eq!(local_of(wire, "svc"), Some("UserService"));
    assert!(
        matches!(local_of(wire, "fallback"), Some("IUserStore" | "MemoryStore")),
        "fallback should be typed; got {:?}",
        local_of(wire, "fallback")
    );

    // The DI call-site shape: bare-field receiver in Register.
    let save = register.calls.iter().find(|c| c.name == "Save").unwrap();
    assert_eq!(save.receiver.as_deref(), Some("_store"));

    // Expression-bodied Flush also calls through the primary-ctor-backed
    // field.
    let flush = func(&file, "Flush");
    let audit_save = flush.calls.iter().find(|c| c.name == "Save").unwrap();
    assert_eq!(audit_save.receiver.as_deref(), Some("_audit"));
}
