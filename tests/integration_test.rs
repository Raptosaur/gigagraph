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
    let sem = index.vectors.sem_vector_of(evens).unwrap().to_vec();
    let hits = index.vectors.top_k(&v, &sem, 3, Some(evens));
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

#[test]
fn di_field_type_narrows_interface_call_to_implementation() {
    let index = index();

    // Publisher.release: `$this->store->persist(...)` — the field's declared
    // type is the SlugStore interface; the hierarchy expands it to the sole
    // implementor, and body-over-signature ranking picks DbSlugStore.
    assert_eq!(
        callee_qname(&index, "release", "persist").as_deref(),
        Some("Acme\\Web::DbSlugStore::persist")
    );
}

#[test]
fn nest_module_provider_pairs_surface_at_extraction() {
    // Extraction-level proof for the @Module deep harvest: the provider
    // object `{provide: Notifier, useClass: EmailNotifier}` sits below the
    // generic depth-2 arg-lit walk; the targeted secondary harvest surfaces
    // it as a keyed Ident pair sharing an index.
    use gigagraph::extract::LitKind;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/monorepo/nestsvc/app.module.ts");
    let source = std::fs::read_to_string(&path).unwrap();
    let spec = gigagraph::lang::spec_for_ext("ts").unwrap();
    let file = gigagraph::extract::extract(spec, &source).unwrap();

    let deco = file
        .functions
        .iter()
        .flat_map(|f| &f.decorations)
        .find(|d| d.name == "Module")
        .expect("@Module decoration extracted");
    let provide = deco
        .arg_lits
        .iter()
        .find(|l| l.key.as_deref() == Some("provide"))
        .expect("provide lit surfaced");
    let use_class = deco
        .arg_lits
        .iter()
        .find(|l| l.key.as_deref() == Some("useClass"))
        .expect("useClass lit surfaced");
    assert_eq!(provide.kind, LitKind::Ident);
    assert_eq!(provide.text, "Notifier");
    assert_eq!(use_class.kind, LitKind::Ident);
    assert_eq!(use_class.text, "EmailNotifier");
    assert_eq!(
        provide.index, use_class.index,
        "pair must share a provider ordinal"
    );
}

#[test]
fn nest_module_provider_binding_disambiguates() {
    let index = index();
    let g = &index.graph;

    // Notifier has TWO implementors (AlertNotifier first by fn id) — only
    // the @Module {provide, useClass} binding names EmailNotifier.
    assert_eq!(
        g.di_bindings.get("Notifier"),
        Some(&vec!["EmailNotifier".to_string()])
    );
    assert_eq!(
        callee_qname(
            &index,
            "nestsvc/dispatch.service::DispatchService::broadcast",
            "send"
        )
        .as_deref(),
        Some("nestsvc/notifiers::EmailNotifier::send")
    );
}

#[test]
fn csharp_addscoped_type_args_bind_container() {
    let index = index();
    let g = &index.graph;

    // `services.AddScoped<IShipStore, DbShipStore>()`: the captured type
    // args land in di_bindings and beat the two-implementor hierarchy
    // fan-out (MemShipStore comes first by fn id).
    assert_eq!(
        g.di_bindings.get("IShipStore"),
        Some(&vec!["DbShipStore".to_string()])
    );
    assert_eq!(
        callee_qname(&index, "Acme.Store::ShipModule::Enqueue", "Persist").as_deref(),
        Some("Acme.Store::DbShipStore::Persist")
    );
}

#[test]
fn two_hop_receiver_narrows_through_field_type() {
    let index = index();
    let g = &index.graph;

    // C#: `load.Store.Stamp()` — load: Crate (local), Crate.Store:
    // CrateStore (field), so the call narrows to CrateStore::Stamp even
    // though LabelPrinter::Stamp comes first by fn id (name-based same-file
    // resolution would pick it).
    assert_eq!(
        callee_qname(&index, "Acme.Store::Dockyard::Seal", "Stamp").as_deref(),
        Some("Acme.Store::CrateStore::Stamp")
    );
    // Two hops through the tables is honest Heuristic, never High.
    let seal = resolve_function_ref(g, "Acme.Store::Dockyard::Seal").unwrap();
    let stamp = g.calls_by_caller[seal as usize]
        .iter()
        .map(|&i| &g.calls[i as usize])
        .find(|c| c.name == "Stamp")
        .expect("Seal calls Stamp");
    match &stamp.resolution {
        Resolution::Internal { confidence, .. } => {
            assert_eq!(*confidence, gigagraph::types::Confidence::Heuristic)
        }
        other => panic!("Stamp not internally resolved: {other:?}"),
    }

    // Go: `d.store.Save()` in gosvc/dispatch.go. Go methods carry no
    // containing_type today, so the two-hop rung finds no OrderStore methods
    // and resolution falls through to the same-file name rung — assert by
    // callee identity (name + file) so this keeps passing if go.rs later
    // gains receiver-based containing_type.
    let flush = g
        .functions
        .iter()
        .find(|f| f.name == "Flush" && f.language == gigagraph::types::Lang::Go)
        .expect("go Flush extracted");
    let save = g.calls_by_caller[flush.id as usize]
        .iter()
        .map(|&i| &g.calls[i as usize])
        .find(|c| c.name == "Save")
        .expect("Flush calls Save");
    match &save.resolution {
        Resolution::Internal { callee, .. } => {
            let f = &g.functions[*callee as usize];
            assert_eq!(f.name, "Save");
            assert_eq!(g.files[f.file_id as usize].path, "gosvc/dispatch.go");
        }
        other => panic!("Save not internally resolved: {other:?}"),
    }
}

#[test]
fn di_container_binding_disambiguates_multiple_implementors() {
    let index = index();

    // SlugStore has TWO implementors (DbSlugStore, CachingSlugStore) — bare
    // hierarchy expansion would be an ambiguous Heuristic. The Laravel-style
    // `$this->app->bind(SlugStore::class, DbSlugStore::class)` names the
    // implementation, so the call resolves to it with confidence intact.
    let g = &index.graph;
    assert_eq!(
        g.di_bindings.get("SlugStore"),
        Some(&vec!["DbSlugStore".to_string()])
    );
    assert_eq!(
        callee_qname(&index, "release", "persist").as_deref(),
        Some("Acme\\Web::DbSlugStore::persist")
    );
}
