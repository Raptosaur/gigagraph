//! End-to-end: index the multi-language monorepo fixture and check the graph,
//! resolution, packages, and similarity vectors.

use gigagraph::api::resolve_function_ref;
use gigagraph::indexer::{Index, build_index};
use gigagraph::types::Resolution;
use std::path::Path;
use std::sync::Mutex;

/// Tests share the fixture's on-disk .gigagraph cache; serialize builds.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

fn index() -> Index {
    let _guard = BUILD_LOCK.lock().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/monorepo");
    build_index(&root, true).expect("index build failed")
}

fn callee_qname(index: &Index, caller: &str, callee_name: &str) -> Option<String> {
    let g = &index.graph;
    let id = resolve_function_ref(g, caller).ok()?;
    for &ci in &g.calls_by_caller[id as usize] {
        let call = &g.calls[ci as usize];
        if call.name == callee_name {
            if let Resolution::Internal { callee, .. } = &call.resolution {
                return Some(g.functions[*callee as usize].qualified_name.clone());
            }
        }
    }
    None
}

fn external_package(index: &Index, caller: &str, callee_name: &str) -> Option<String> {
    let g = &index.graph;
    let id = resolve_function_ref(g, caller).ok()?;
    for &ci in &g.calls_by_caller[id as usize] {
        let call = &g.calls[ci as usize];
        if call.name == callee_name {
            if let Resolution::External { package } = &call.resolution {
                return Some(package.clone());
            }
        }
    }
    None
}

#[test]
fn cross_file_resolution_all_languages() {
    let index = index();
    let g = &index.graph;

    // Every language contributed functions.
    for lang in [
        "typescript",
        "javascript",
        "c",
        "java",
        "kotlin",
        "swift",
        "rust",
        "bash",
        "python",
        "go",
        "ruby",
        "cpp",
        "csharp",
        "php",
    ] {
        assert!(
            index
                .stats
                .functions_by_language
                .get(lang)
                .copied()
                .unwrap_or(0)
                > 0,
            "no functions extracted for {lang}; stats: {:?}",
            index.stats.functions_by_language
        );
    }

    // TS: import-directed cross-file resolution.
    assert_eq!(
        callee_qname(&index, "listUsers", "queryRows").as_deref(),
        Some("web/src/db::queryRows")
    );
    // C: header-mediated cross-file resolution.
    assert_eq!(
        callee_qname(&index, "scale", "clamp_int").as_deref(),
        Some("native/util::clamp_int")
    );
    // Java: same-package + static receiver resolution.
    assert_eq!(
        callee_qname(&index, "com.acme.app::Main::main", "shout").as_deref(),
        Some("com.acme.app::Util::shout")
    );
    // Kotlin: same-package resolution.
    assert_eq!(
        callee_qname(&index, "greet", "titleCase").as_deref(),
        Some("com.acme.lib::titleCase")
    );
    // Swift: global name resolution.
    assert_eq!(
        callee_qname(&index, "run", "formatScore").as_deref(),
        Some("ios/Sources/Format::formatScore")
    );
    // Rust: crate::-use cross-file resolution in a nested crate.
    assert_eq!(
        callee_qname(&index, "rustcli/src/main::main", "parse_flags").as_deref(),
        Some("rustcli/src/parse::parse_flags")
    );
    // Python: absolute dotted import resolution.
    assert_eq!(
        callee_qname(&index, "pysvc/app::publish", "slugify").as_deref(),
        Some("pysvc/helpers::slugify")
    );
    // Go: same-package resolution.
    assert_eq!(
        callee_qname(&index, "Greet", "Normalize").as_deref(),
        Some("gosvc::Normalize")
    );
    // Bash: sourced-file resolution.
    assert_eq!(
        callee_qname(&index, "deploy", "log_info").as_deref(),
        Some("scripts/lib::log_info")
    );
    // Ruby: module receiver resolution across require_relative.
    assert_eq!(
        callee_qname(&index, "receipt", "currency").as_deref(),
        Some("rbsvc/helper::Format::currency")
    );
    // C++: method resolution across an include.
    assert_eq!(
        callee_qname(&index, "describe", "area").as_deref(),
        Some("cppsvc/shape::Shape::area")
    );
    // C#: static receiver resolution within a namespace.
    assert_eq!(
        callee_qname(&index, "Register", "Normalize").as_deref(),
        Some("Acme.Store::Util::Normalize")
    );
    // PHP: same-namespace function resolution.
    assert_eq!(
        callee_qname(&index, "Acme\\Web::Service::publish", "formatSlug").as_deref(),
        Some("Acme\\Web::formatSlug")
    );

    // Reverse edges: queryRows knows listUsers calls it.
    let qr = resolve_function_ref(g, "queryRows").unwrap();
    let callers = g.callers_of.get(&qr).expect("queryRows has callers");
    assert!(
        callers
            .iter()
            .any(|&ci| g.functions[g.calls[ci as usize].caller as usize].name == "listUsers")
    );
}

#[test]
fn external_packages_attributed() {
    let index = index();

    // TS: `express()` via default import.
    assert_eq!(
        external_package(&index, "startApi", "express").as_deref(),
        Some("express")
    );
    // JS: `axios.get` via require receiver.
    assert_eq!(
        external_package(&index, "ping", "get").as_deref(),
        Some("axios")
    );
    // C: printf is unresolvable by name; stdio.h shows up as an import package.
    let native_main = index
        .graph
        .files
        .iter()
        .find(|f| f.path == "native/main.c")
        .unwrap();
    assert!(
        native_main
            .imports
            .iter()
            .any(|i| i.system && i.external_package.as_deref() == Some("stdio.h"))
    );
    // Java: System.out.println -> builtin.
    assert_eq!(
        external_package(&index, "com.acme.app::Main::main", "println").as_deref(),
        Some("builtin:System")
    );

    // Package roll-up includes the npm deps.
    let packages: Vec<&str> = index
        .graph
        .package_calls
        .keys()
        .map(|s| s.as_str())
        .collect();
    for expected in ["express", "axios", "pg"] {
        assert!(
            packages.contains(&expected),
            "missing package {expected}; got {packages:?}"
        );
    }
}

#[test]
fn similarity_finds_structural_twin() {
    let index = index();
    let g = &index.graph;

    // sumEvens (db.ts) and sumOdds (api.ts) are structural twins:
    // loop + branch + accumulator. Each must rank the other first.
    let evens = resolve_function_ref(g, "sumEvens").unwrap();
    let odds = resolve_function_ref(g, "sumOdds").unwrap();
    let v = index.vectors.vector_of(evens).unwrap().to_vec();
    let hits = index.vectors.top_k(&v, 3, Some(evens));
    assert!(!hits.is_empty(), "no similarity hits for sumEvens");
    assert_eq!(
        hits[0].0, odds,
        "sumOdds should be sumEvens' nearest neighbor; got {}",
        g.functions[hits[0].0 as usize].qualified_name
    );
    assert!(hits[0].1 > 0.5, "similarity too low: {}", hits[0].1);
}

#[test]
fn incremental_reindex_reuses_cache() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/monorepo");
    let first = build_index(&root, true).expect("first build");
    assert_eq!(first.stats.cached_files, 0);
    let second = build_index(&root, false).expect("second build");
    assert_eq!(
        second.stats.parsed_files, 0,
        "second build should be fully cached"
    );
    assert_eq!(second.stats.functions, first.stats.functions);
    assert_eq!(second.stats.calls, first.stats.calls);
}
