mod common;

use gigagraph::extract::LitKind;
use common::*;

#[test]
fn extracts_objc_methods_functions_calls_and_imports() {
    let file = extract_fixture("m", "objc/PaymentsModule.m");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // ObjC methods surface under their FIRST selector segment (`logAll` for
    // `logAll:prefix:`); plain C functions in the .m file extract as in C.
    for expected in [
        "clamp_amount",
        "parseAmount",
        "logAll",
        "clearToken",
        "shared",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // The grammar exposes no `name` field on class_implementation; the
    // extractor's positional fallback (first identifier child) recovers the
    // class name — but ONLY where RCT macro ERROR-recovery hasn't shattered
    // the @implementation node, so no blanket assertion here (the macro-free
    // fixture asserts positive resolution).
    assert_eq!(file.package, None);

    // No parameter-list wrapper node exists for ObjC methods (same limitation
    // as Swift): methods extract with 0 params; C functions keep theirs.
    assert_eq!(func(&file, "clamp_amount").param_count, 1);
    assert_eq!(func(&file, "parseAmount").param_count, 0);
    assert_eq!(func(&file, "logAll").param_count, 0);

    // Message sends: receiver captured, callee = first selector segment.
    assert_calls(
        func(&file, "parseAmount"),
        &["alloc", "init", "setNumberStyle", "numberFromString"],
    );
    let alloc = func(&file, "parseAmount")
        .calls
        .iter()
        .find(|c| c.name == "alloc")
        .unwrap();
    assert_eq!(alloc.receiver.as_deref(), Some("NSNumberFormatter"));
    // Nested sends keep the bracketed inner send as receiver text.
    let init = func(&file, "parseAmount")
        .calls
        .iter()
        .find(|c| c.name == "init")
        .unwrap();
    assert_eq!(init.receiver.as_deref(), Some("[NSNumberFormatter alloc]"));

    // Multi-segment send `[self logAll:... prefix:...]` yields ONE call named
    // after the first segment — no phantom `prefix` call. Message arguments
    // are loose children (no argument_list), so arg_count stays 0.
    let log_all = func(&file, "clearToken")
        .calls
        .iter()
        .find(|c| c.name == "logAll")
        .unwrap();
    assert_eq!(log_all.receiver.as_deref(), Some("self"));
    assert_eq!(log_all.arg_count, 0);
    assert!(
        !func(&file, "clearToken")
            .calls
            .iter()
            .any(|c| c.name == "prefix"),
        "second selector segment must not become its own call"
    );

    // Plain C calls inside a method body still carry arg counts.
    assert_calls(func(&file, "clearToken"), &["printf", "clamp_amount"]);
    let nslog = func(&file, "logAll")
        .calls
        .iter()
        .find(|c| c.name == "NSLog")
        .unwrap();
    assert_eq!(nslog.arg_count, 3);

    // `#import "..."` / `#import <...>` / `#include <...>` behave like C
    // includes; `@import CoreLocation;` comes through module_import.
    let paths = import_paths(&file);
    for p in [
        "Foundation/Foundation.h",
        "React/RCTBridgeModule.h",
        "PaymentsModule.h",
        "stdio.h",
        "CoreLocation",
    ] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }
    for sys in [
        "Foundation/Foundation.h",
        "React/RCTBridgeModule.h",
        "stdio.h",
    ] {
        let imp = file.imports.iter().find(|i| i.path == sys).unwrap();
        assert!(imp.system, "<{sys}> should be a system import");
    }
    for local in ["PaymentsModule.h", "CoreLocation"] {
        let imp = file.imports.iter().find(|i| i.path == local).unwrap();
        assert!(!imp.system, "{local} should not be a system import");
    }
    for imp in &file.imports {
        assert!(imp.names.is_empty(), "ObjC imports bind no names");
    }
}

/// Pins the RCT_EXPORT_* macro outcomes the `bridge_map` feature depends on.
/// The grammar cannot parse the macros as ObjC; ERROR recovery yields stable
/// shapes the query turns into decorations (typed selectors) or a function
/// plus a decoration (parameterless selectors).
#[test]
fn rct_export_macros_yield_functions_or_decorations() {
    let file = extract_fixture("m", "objc/PaymentsModule.m");

    // Typed selector `RCT_EXPORT_METHOD(processPayment:... resolver:...
    // rejecter:...)`: NO function is extracted for it; instead a decoration
    // named RCT_EXPORT_METHOD carries the selector's first segment as an
    // identifier ArgLit (bridge_map outcome (b)).
    let all_decos: Vec<_> = file
        .functions
        .iter()
        .flat_map(|f| f.decorations.iter())
        .collect();
    let process_deco = all_decos
        .iter()
        .find(|d| {
            d.name == "RCT_EXPORT_METHOD"
                && d.arg_lits
                    .iter()
                    .any(|a| a.kind == LitKind::Ident && a.text == "processPayment")
        })
        .unwrap_or_else(|| {
            panic!(
                "no RCT_EXPORT_METHOD decoration carrying `processPayment`; decos: {:?}",
                all_decos
                    .iter()
                    .map(|d| (d.name.as_str(), d.line))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(process_deco.line, 22);

    // Parameterless selector `RCT_EXPORT_METHOD(reset)`: ERROR recovery is a
    // clean bogus function_definition, so `reset` extracts as a real function
    // (bridge_map outcome (a)) AND carries a same-match RCT_EXPORT_METHOD
    // decoration; its body calls attribute correctly.
    let reset = func(&file, "reset");
    assert!(
        reset
            .decorations
            .iter()
            .any(|d| d.name == "RCT_EXPORT_METHOD"),
        "reset should carry an RCT_EXPORT_METHOD decoration; got {:?}",
        reset
            .decorations
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(reset.signature, "RCT_EXPORT_METHOD(reset)");
    let clear = reset.calls.iter().find(|c| c.name == "clearToken").unwrap();
    assert_eq!(clear.receiver.as_deref(), Some("self"));

    // RCT_EXPORT_MODULE(); is also recovered as a decoration.
    assert!(
        all_decos.iter().any(|d| d.name == "RCT_EXPORT_MODULE"),
        "missing RCT_EXPORT_MODULE decoration"
    );

    // The typed RCT method's BODY is parse wreckage not contained by any
    // captured function, so its calls land on the synthetic (toplevel)
    // function — pinned so a grammar improvement is noticed.
    let top = func(&file, "(toplevel)");
    assert_calls(top, &["parseAmount", "reject", "resolve"]);
    let parse_call = top.calls.iter().find(|c| c.name == "parseAmount").unwrap();
    assert_eq!(parse_call.receiver.as_deref(), Some("self"));
    // ObjC `@"..."` literals are plain string_literal nodes; the `@` prefix
    // is stripped when harvesting argument literals.
    let reject = top.calls.iter().find(|c| c.name == "reject").unwrap();
    assert!(
        reject
            .arg_lits
            .iter()
            .any(|a| a.kind == LitKind::Str && a.text == "bad_amount"),
        "reject() should carry the @\"bad_amount\" string literal; got {:?}",
        reject.arg_lits
    );
}

#[test]
fn plain_objc_file_extracts_c_and_methods() {
    let file = extract_fixture("m", "objc/geometry.m");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // Positional containing_type fallback: methods inside the macro-free
    // @implementation resolve to the class; free C functions do not.
    assert_eq!(
        func(&file, "area").containing_type.as_deref(),
        Some("GeoBox")
    );
    assert_eq!(func(&file, "geo_distance").containing_type, None);

    for expected in ["square", "geo_distance", "area", "contains", "describe"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }
    // Clean parse, no macros: every call is inside a function.
    assert!(
        !names.contains(&"(toplevel)"),
        "unexpected (toplevel); got {names:?}"
    );

    assert_eq!(func(&file, "square").param_count, 1);
    assert_eq!(func(&file, "geo_distance").param_count, 4);
    assert_calls(func(&file, "geo_distance"), &["sqrt", "square"]);

    // `contains:y:` surfaces as `contains` (first selector segment).
    assert_eq!(
        func(&file, "contains").features.get("flow:branches"),
        Some(&1) // if
    );
    assert_eq!(
        func(&file, "describe").features.get("flow:loops"),
        Some(&1) // do-while
    );

    let fmt_call = func(&file, "describe")
        .calls
        .iter()
        .find(|c| c.name == "stringWithFormat")
        .unwrap();
    assert_eq!(fmt_call.receiver.as_deref(), Some("NSString"));
    assert_calls(func(&file, "describe"), &["area", "uppercaseString"]);

    let geo = file
        .imports
        .iter()
        .find(|i| i.path == "geometry.h")
        .unwrap();
    assert!(!geo.system);
    let math = file.imports.iter().find(|i| i.path == "math.h").unwrap();
    assert!(math.system);
}
