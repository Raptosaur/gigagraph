mod common;

use common::*;

#[test]
fn extracts_cpp_free_functions_templates_and_calls() {
    let file = extract_fixture("cpp", "cpp/main.cpp");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // In-struct constructor/destructor/method, template function, static free
    // functions, function-pointer parameter, and main.
    for expected in [
        "Timer",
        "~Timer",
        "tick",
        "clamp_min",
        "total_sides",
        "describe",
        "apply_op",
        "main",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // C++ has no toplevel calls in these fixtures, and no package declaration.
    assert!(
        !names.contains(&"(toplevel)"),
        "unexpected (toplevel); got {names:?}"
    );
    assert_eq!(file.package, None);

    // Struct members know their struct; free functions have no type.
    for member in ["Timer", "~Timer", "tick"] {
        assert_eq!(
            func(&file, member).containing_type.as_deref(),
            Some("Timer"),
            "{member} should live in Timer"
        );
    }
    for free in ["clamp_min", "total_sides", "describe", "apply_op", "main"] {
        assert_eq!(
            func(&file, free).containing_type,
            None,
            "{free} should be free"
        );
    }

    // Param counts (the function-pointer parameter counts as one slot).
    assert_eq!(func(&file, "Timer").param_count, 0);
    assert_eq!(func(&file, "clamp_min").param_count, 2);
    assert_eq!(func(&file, "total_sides").param_count, 1);
    assert_eq!(func(&file, "describe").param_count, 1);
    assert_eq!(func(&file, "apply_op").param_count, 3);
    assert_eq!(func(&file, "main").param_count, 0);

    // Call attribution. `clamp_min` appears both as a plain call and as an
    // explicit template call `clamp_min<int>(...)`; `Shape` comes from the
    // qualified constructor-style call `geo::Shape(4)` and `new geo::Shape(5)`.
    assert_calls(
        func(&file, "main"),
        &[
            "push_back",
            "unit",
            "Shape",
            "describe",
            "front",
            "tick",
            "clamp_min",
            "total_sides",
            "sides",
            "printf",
        ],
    );
    assert_calls(func(&file, "describe"), &["sides", "printf"]);
    assert_calls(func(&file, "total_sides"), &["sides"]);
    // Explicitly dereferenced function pointer: `(*op)(a, b)`.
    assert!(has_call(func(&file, "apply_op"), "op"));
    assert_calls(func(&file, "~Timer"), &["printf"]);

    // Receivers: member calls (`obj.m`, `ptr->m`), namespace-qualified calls
    // (`std::printf`), and class-qualified statics (`geo::Shape::unit` — the
    // class, not the namespace, is the receiver).
    let main_fn = func(&file, "main");
    let push = main_fn
        .calls
        .iter()
        .find(|c| c.name == "push_back")
        .unwrap();
    assert_eq!(push.receiver.as_deref(), Some("shapes"));
    assert_eq!(push.arg_count, 1);
    let tick = main_fn.calls.iter().find(|c| c.name == "tick").unwrap();
    assert_eq!(tick.receiver.as_deref(), Some("timer"));
    let unit = main_fn.calls.iter().find(|c| c.name == "unit").unwrap();
    assert_eq!(unit.receiver.as_deref(), Some("Shape"));
    assert_eq!(unit.arg_count, 0);
    let ctor = main_fn.calls.iter().find(|c| c.name == "Shape").unwrap();
    assert_eq!(ctor.receiver.as_deref(), Some("geo"));
    assert_eq!(ctor.arg_count, 1);
    let printf = main_fn.calls.iter().find(|c| c.name == "printf").unwrap();
    assert_eq!(printf.receiver.as_deref(), Some("std"));
    assert_eq!(printf.arg_count, 3);
    let heap_sides = main_fn.calls.iter().find(|c| c.name == "sides").unwrap();
    assert_eq!(heap_sides.receiver.as_deref(), Some("heap")); // heap->sides()
    let arrow = func(&file, "describe")
        .calls
        .iter()
        .find(|c| c.name == "sides")
        .unwrap();
    assert_eq!(arrow.receiver.as_deref(), Some("shape")); // shape->sides()

    // Both clamp_min call forms attributed to main.
    assert_eq!(main_fn.features.get("call:clamp_min"), Some(&2));

    // Control flow: range-for and while count as loops, if/ternary as branches.
    assert_eq!(
        func(&file, "total_sides").features.get("flow:loops"),
        Some(&1) // range-for
    );
    assert_eq!(main_fn.features.get("flow:loops"), Some(&1)); // while
    assert_eq!(
        func(&file, "describe").features.get("flow:branches"),
        Some(&1) // if
    );
    assert_eq!(
        func(&file, "clamp_min").features.get("flow:branches"),
        Some(&1) // ternary
    );

    // Includes: quote vs angle.
    let paths = import_paths(&file);
    for p in ["shape.hpp", "cstdio", "vector"] {
        assert!(paths.contains(&p), "missing include {p}; got {paths:?}");
    }
    for sys in ["cstdio", "vector"] {
        let imp = file.imports.iter().find(|i| i.path == sys).unwrap();
        assert!(imp.system, "<{sys}> should be a system include");
    }
    let local = file.imports.iter().find(|i| i.path == "shape.hpp").unwrap();
    assert!(
        !local.system,
        "\"shape.hpp\" should not be a system include"
    );
    for imp in &file.imports {
        assert!(
            imp.names.is_empty(),
            "C++ includes bind no names; {} got {:?}",
            imp.path,
            imp.names
        );
    }
}

#[test]
fn extracts_out_of_class_definitions_and_namespaces() {
    let file = extract_fixture("cpp", "cpp/shape.cpp");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Free function in a namespace, out-of-class destructor/methods
    // (`Shape::~Shape`, `Shape::area`, `Shape::label`), pointer-returning free
    // function, and a doubly-qualified definition at global scope
    // (`geo::Shape::unit`) — always the rightmost name.
    for expected in ["unit_area", "~Shape", "area", "label", "shape_kind", "unit"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }
    assert!(!names.contains(&"(toplevel)"), "unexpected (toplevel)");

    // Known limitation: for out-of-class definitions the class in the
    // qualifier is not an AST ancestor, so containing_type falls back to the
    // enclosing namespace (or nothing at global scope) — never the class.
    for in_ns in ["unit_area", "~Shape", "area", "label", "shape_kind"] {
        assert_eq!(
            func(&file, in_ns).containing_type.as_deref(),
            Some("geo"),
            "{in_ns} should report namespace geo"
        );
    }
    assert_eq!(func(&file, "unit").containing_type, None);

    assert_eq!(func(&file, "unit_area").param_count, 1);
    assert_eq!(func(&file, "area").param_count, 0);
    assert_eq!(func(&file, "shape_kind").param_count, 1);

    // `std::fabs(...)` resolves through the std receiver; nested call to the
    // local helper is attributed alongside it.
    assert_calls(func(&file, "area"), &["fabs", "unit_area"]);
    let fabs = func(&file, "area")
        .calls
        .iter()
        .find(|c| c.name == "fabs")
        .unwrap();
    assert_eq!(fabs.receiver.as_deref(), Some("std"));
    let str_call = func(&file, "label")
        .calls
        .iter()
        .find(|c| c.name == "str")
        .unwrap();
    assert_eq!(str_call.receiver.as_deref(), Some("out")); // out.str()
    let sides = func(&file, "shape_kind")
        .calls
        .iter()
        .find(|c| c.name == "sides")
        .unwrap();
    assert_eq!(sides.receiver.as_deref(), Some("s")); // s.sides()
    assert_calls(func(&file, "unit"), &["Shape"]); // geo::Shape(3)

    // switch + ternary in shape_kind.
    assert_eq!(
        func(&file, "shape_kind").features.get("flow:branches"),
        Some(&2)
    );

    // Namespace identifiers land in the semantic feature bag.
    assert!(func(&file, "unit").features.contains_key("id:geo"));

    let paths = import_paths(&file);
    for p in ["shape.hpp", "../util/format.hpp", "cmath", "sstream"] {
        assert!(paths.contains(&p), "missing include {p}; got {paths:?}");
    }
    for sys in ["cmath", "sstream"] {
        assert!(file.imports.iter().find(|i| i.path == sys).unwrap().system);
    }
    for local in ["shape.hpp", "../util/format.hpp"] {
        assert!(
            !file
                .imports
                .iter()
                .find(|i| i.path == local)
                .unwrap()
                .system
        );
    }
}

#[test]
fn header_class_with_inline_methods() {
    let file = extract_fixture("hpp", "cpp/shape.hpp");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Only definitions count: the inline constructor and methods. Prototypes
    // (`~Shape();`, `virtual double area() const;`, `static Shape unit();`)
    // are declarations, not definitions.
    for expected in ["Shape", "sides", "scaled_area"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }
    for decl_only in ["~Shape", "area", "label", "unit"] {
        assert!(
            !names.contains(&decl_only),
            "{decl_only} is only declared; got {names:?}"
        );
    }
    assert!(!names.contains(&"(toplevel)"), "unexpected (toplevel)");

    // Inline members know their class (nearest type wins over the namespace).
    for member in ["Shape", "sides", "scaled_area"] {
        assert_eq!(
            func(&file, member).containing_type.as_deref(),
            Some("Shape"),
            "{member} should live in class Shape"
        );
    }

    assert_eq!(func(&file, "Shape").param_count, 1);
    assert_eq!(func(&file, "sides").param_count, 0);
    assert_eq!(func(&file, "scaled_area").param_count, 1);

    // Unqualified method-to-method call plus the ternary branch.
    assert_calls(func(&file, "scaled_area"), &["area"]);
    assert_eq!(
        func(&file, "scaled_area").features.get("flow:branches"),
        Some(&1)
    );

    // The member-initializer list `: sides_(sides)` must not fabricate a call.
    assert!(!has_call(func(&file, "Shape"), "sides_"));

    for sys in ["string", "cstdint"] {
        let imp = file.imports.iter().find(|i| i.path == sys).unwrap();
        assert!(imp.system, "<{sys}> should be a system include");
    }
}
