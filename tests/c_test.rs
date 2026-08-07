mod common;

use common::*;

#[test]
fn extracts_c_functions() {
    let file = extract_fixture("c", "c/vec.c");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Plain, static, pointer-returning (`Vec *`, `char *`, `char **`,
    // `const char *`), varargs, and main.
    for expected in [
        "default_cap",
        "grow_cap",
        "vec_new",
        "vec_to_string",
        "split_lines",
        "log_msg",
        "apply",
        "apply_deref",
        "dispatch",
        "backend_flush",
        "count_markers",
        "status_name",
        "main",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // C has no toplevel calls, so no synthetic "(toplevel)" function.
    assert!(
        !names.contains(&"(toplevel)"),
        "unexpected (toplevel); got {names:?}"
    );

    // All C functions are free functions, and C has no package declaration.
    for f in &file.functions {
        assert_eq!(
            f.containing_type, None,
            "C function `{}` should have no containing type",
            f.name
        );
    }
    assert_eq!(file.package, None);

    // Param counts: `(void)` means zero; `...` counts as one parameter slot.
    assert_eq!(func(&file, "default_cap").param_count, 0);
    assert_eq!(func(&file, "grow_cap").param_count, 1);
    assert_eq!(func(&file, "vec_new").param_count, 1);
    assert_eq!(func(&file, "split_lines").param_count, 2);
    assert_eq!(func(&file, "log_msg").param_count, 2);
    assert_eq!(func(&file, "apply").param_count, 2);
    assert_eq!(func(&file, "main").param_count, 2);

    // Call attribution per function.
    assert_calls(func(&file, "vec_new"), &["alloc", "default_cap"]);
    assert_calls(func(&file, "vec_to_string"), &["malloc", "snprintf"]);
    // `calloc(default_cap(), ...)`: nested call captured alongside the outer.
    assert_calls(
        func(&file, "split_lines"),
        &["calloc", "default_cap", "strndup"],
    );
    assert_calls(func(&file, "log_msg"), &["va_start", "vfprintf", "va_end"]);
    // Call through a function-pointer parameter: `op(x)`.
    assert_calls(func(&file, "apply"), &["op"]);
    // Explicitly dereferenced function pointer: `(*op)(x)`.
    assert!(has_call(func(&file, "apply_deref"), "op"));
    assert_calls(func(&file, "count_markers"), &["putchar"]);
    // `log_msg("...", grow_cap(default_cap()))` and
    // `printf("...", status_name(dispatch(h, argc)))`: every level attributed
    // to main.
    assert_calls(
        func(&file, "main"),
        &[
            "log_msg",
            "grow_cap",
            "default_cap",
            "printf",
            "status_name",
            "dispatch",
            "apply",
            "apply_deref",
        ],
    );

    // Struct-member function-pointer calls carry their receiver.
    let alloc = func(&file, "vec_new")
        .calls
        .iter()
        .find(|c| c.name == "alloc")
        .unwrap();
    assert_eq!(alloc.receiver.as_deref(), Some("a")); // a->alloc(...)
    assert_eq!(alloc.arg_count, 1);
    let on_event = func(&file, "dispatch")
        .calls
        .iter()
        .find(|c| c.name == "on_event")
        .unwrap();
    assert_eq!(on_event.receiver.as_deref(), Some("h")); // h.on_event(...)
    assert_eq!(on_event.arg_count, 1);
    let flush = func(&file, "backend_flush")
        .calls
        .iter()
        .find(|c| c.name == "flush")
        .unwrap();
    assert_eq!(flush.receiver.as_deref(), Some("b->ops")); // b->ops->flush(...)
    let log_call = func(&file, "main")
        .calls
        .iter()
        .find(|c| c.name == "log_msg")
        .unwrap();
    assert_eq!(log_call.receiver, None);
    assert_eq!(log_call.arg_count, 2);

    // Control flow lands in the feature bag.
    let markers = func(&file, "count_markers");
    assert_eq!(markers.features.get("flow:loops"), Some(&3)); // while + for + do
    assert_eq!(markers.features.get("flow:branches"), Some(&1)); // if
    assert!(markers.features.contains_key("call:putchar"));
    assert_eq!(
        func(&file, "vec_to_string").features.get("flow:loops"),
        Some(&1)
    ); // for
    assert_eq!(
        func(&file, "grow_cap").features.get("flow:branches"),
        Some(&1)
    ); // ternary
    assert_eq!(
        func(&file, "status_name").features.get("flow:branches"),
        Some(&2) // switch + ternary
    );

    // Includes: `<...>` sets the system flag, `"..."` does not; C binds no
    // local names.
    let paths = import_paths(&file);
    for p in [
        "stdio.h",
        "stdlib.h",
        "stdarg.h",
        "string.h",
        "util.h",
        "../shared/compat.h",
    ] {
        assert!(paths.contains(&p), "missing include {p}; got {paths:?}");
    }
    for sys in ["stdio.h", "stdlib.h", "stdarg.h", "string.h"] {
        let imp = file.imports.iter().find(|i| i.path == sys).unwrap();
        assert!(imp.system, "<{sys}> should be a system include");
    }
    for local in ["util.h", "../shared/compat.h"] {
        let imp = file.imports.iter().find(|i| i.path == local).unwrap();
        assert!(!imp.system, "\"{local}\" should not be a system include");
    }
    for imp in &file.imports {
        assert!(
            imp.names.is_empty(),
            "C includes bind no names; {} got {:?}",
            imp.path,
            imp.names
        );
    }
}

#[test]
fn triple_pointer_return_uses_deepest_declarator_nesting() {
    // `double ***alloc_cube(...)` nests function_declarator under three
    // pointer_declarator levels — the deepest form the query supports.
    let file = extract_fixture("c", "c/cube.c");
    let cube = func(&file, "alloc_cube");
    assert_eq!(cube.param_count, 1);
    assert_eq!(cube.containing_type, None);
    assert_calls(cube, &["malloc", "calloc"]);
    assert_eq!(cube.features.get("flow:loops"), Some(&2)); // nested for loops
    assert!(
        !file.functions.iter().any(|f| f.name == "(toplevel)"),
        "unexpected (toplevel)"
    );
}

#[test]
fn header_with_declarations_only_yields_no_functions() {
    let file = extract_fixture("h", "c/util.h");

    // Prototypes, macros, and struct declarations are not definitions, and C
    // has no toplevel calls — no functions at all, not even "(toplevel)".
    assert!(
        file.functions.is_empty(),
        "expected no functions; got {:?}",
        file.functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );

    // The include nested inside the #ifndef guard is still extracted.
    let stddef = file.imports.iter().find(|i| i.path == "stddef.h").unwrap();
    assert!(stddef.system);
}
