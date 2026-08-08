//! Validation against real open-source applications.
//!
//! Fixtures prove a construct is *handled*; this proves the handling survives
//! code nobody wrote for us — vendored trees, generated files, dialects, and
//! scale. Every repo in `tests/corpus.json` is cloned at a pinned commit by
//! `scripts/fetch-corpus.sh` into a gitignored directory.
//!
//! These tests are `#[ignore]`d: they need a network fetch first, and they
//! take minutes. Run them with
//!
//! ```text
//! scripts/fetch-corpus.sh
//! cargo test --test corpus_test -- --ignored --nocapture
//! ```
//!
//! A repo that has not been fetched is skipped, not failed, so a partial
//! corpus still validates what is present. Expectations are FLOORS: a corpus
//! repo that grows must not break the build, but one where detection
//! collapses must.

use gigagraph::api::AppState;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

struct Repo {
    name: String,
    why: String,
    dir: PathBuf,
    languages: Vec<String>,
    min_files: u64,
    min_functions: u64,
    min_endpoints: u64,
    min_test_cases: u64,
    frameworks: Vec<String>,
    /// Floor for (internal + external) / all call sites. Per-repo because
    /// resolvability is a property of the ecosystem, not of the resolver:
    /// a Rails app that autoloads every constant gives the static resolver
    /// almost nothing to work with.
    min_resolution_pct: f64,
}

fn manifest() -> Vec<Repo> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(root.join("tests/corpus.json")).expect("read corpus.json");
    let data: Value = serde_json::from_str(&raw).expect("parse corpus.json");
    let base = root.join(data["dir"].as_str().expect("dir"));
    data["repos"]
        .as_array()
        .expect("repos")
        .iter()
        .map(|r| {
            let name = r["name"].as_str().expect("name").to_string();
            Repo {
                dir: base.join(&name),
                name,
                why: r["why"].as_str().unwrap_or_default().to_string(),
                languages: strings(&r["languages"]),
                min_files: r["min_files"].as_u64().unwrap_or(0),
                min_functions: r["min_functions"].as_u64().unwrap_or(0),
                min_endpoints: r["min_endpoints"].as_u64().unwrap_or(0),
                min_test_cases: r["min_test_cases"].as_u64().unwrap_or(0),
                frameworks: strings(&r["frameworks"]),
                min_resolution_pct: r["min_resolution_pct"].as_f64().unwrap_or(25.0),
            }
        })
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Index a corpus repo once and hand back the state for further queries.
fn index(repo: &Repo) -> Option<(AppState, Value)> {
    if !repo.dir.is_dir() {
        eprintln!(
            "skip {:22} (not fetched — scripts/fetch-corpus.sh {})",
            repo.name, repo.name
        );
        return None;
    }
    let mut state = AppState::new(repo.dir.clone());
    let out = state
        .dispatch("index_stats", &json!({}))
        .unwrap_or_else(|e| panic!("{}: index_stats failed: {e:#}", repo.name));
    // index_stats nests the counters under "stats".
    let stats = out["stats"].clone();
    Some((state, stats))
}

fn fetched(repos: &[Repo]) -> usize {
    repos.iter().filter(|r| r.dir.is_dir()).count()
}

#[test]
#[ignore = "needs scripts/fetch-corpus.sh"]
fn indexes_real_applications() {
    let repos = manifest();
    assert!(
        fetched(&repos) > 0,
        "no corpus repos fetched; run scripts/fetch-corpus.sh first"
    );

    for repo in &repos {
        let Some((_, stats)) = index(repo) else {
            continue;
        };
        let files = stats["files"].as_u64().unwrap_or(0);
        let functions = stats["functions"].as_u64().unwrap_or(0);
        eprintln!(
            "{:22} files={files} functions={functions}  ({})",
            repo.name, repo.why
        );

        assert!(
            files >= repo.min_files,
            "{}: indexed {files} files, floor is {}",
            repo.name,
            repo.min_files
        );
        assert!(
            functions >= repo.min_functions,
            "{}: extracted {functions} functions, floor is {}",
            repo.name,
            repo.min_functions
        );

        // The languages the repo is *about* must actually be represented.
        let by_lang = stats["functions_by_language"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for lang in &repo.languages {
            let n = by_lang.get(lang).and_then(Value::as_u64).unwrap_or(0);
            assert!(
                n > 0,
                "{}: no {lang} functions extracted; got {:?}",
                repo.name,
                by_lang.keys().collect::<Vec<_>>()
            );
        }

        // Call resolution on real code: most call sites should land somewhere
        // (internal or a named external package). A collapse here means the
        // resolver broke, not that the repo changed.
        let internal = stats["resolved_internal"].as_u64().unwrap_or(0);
        let external = stats["resolved_external"].as_u64().unwrap_or(0);
        let unresolved = stats["unresolved"].as_u64().unwrap_or(0);
        let total = internal + external + unresolved;
        if total > 0 {
            let resolved_pct = (internal + external) as f64 * 100.0 / total as f64;
            eprintln!(
                "{:22} resolution: {resolved_pct:.0}% ({internal} internal, {external} external, {unresolved} unresolved)",
                ""
            );
            assert!(
                resolved_pct >= repo.min_resolution_pct,
                "{}: only {resolved_pct:.0}% of call sites resolved, floor is {:.0}%",
                repo.name,
                repo.min_resolution_pct
            );
        }
    }
}

#[test]
#[ignore = "needs scripts/fetch-corpus.sh"]
fn finds_real_test_suites() {
    let repos = manifest();
    assert!(fetched(&repos) > 0, "no corpus repos fetched");

    for repo in &repos {
        if repo.min_test_cases == 0 && repo.frameworks.is_empty() {
            continue;
        }
        let Some((mut state, _)) = index(repo) else {
            continue;
        };
        let out = state
            .dispatch("list_tests", &json!({ "limit": 500 }))
            .unwrap_or_else(|e| panic!("{}: list_tests failed: {e:#}", repo.name));

        let cases = out["total_cases"].as_u64().unwrap_or(0);
        let frameworks = out["frameworks"].as_object().cloned().unwrap_or_default();
        eprintln!(
            "{:22} cases={cases} frameworks={:?}",
            repo.name,
            frameworks.keys().collect::<Vec<_>>()
        );

        assert!(
            cases >= repo.min_test_cases,
            "{}: found {cases} test cases, floor is {}",
            repo.name,
            repo.min_test_cases
        );
        for framework in &repo.frameworks {
            assert!(
                frameworks.contains_key(framework),
                "{}: expected {framework} tests; got {:?}",
                repo.name,
                frameworks.keys().collect::<Vec<_>>()
            );
        }

        // Names must be real, not placeholders: a detector that reports empty
        // or duplicated names is worse than one that reports nothing.
        let names: Vec<&str> = out["files"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|f| f["tests"].as_array().unwrap())
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(
            names.iter().all(|n| !n.trim().is_empty()),
            "{}: blank test names",
            repo.name
        );
        let distinct: std::collections::HashSet<&&str> = names.iter().collect();
        assert!(
            distinct.len() * 4 >= names.len(),
            "{}: {} of {} test names are duplicates — detection is probably \
             latching onto the wrong node",
            repo.name,
            names.len() - distinct.len(),
            names.len()
        );
    }
}

#[test]
#[ignore = "needs scripts/fetch-corpus.sh"]
fn finds_real_api_surfaces() {
    let repos = manifest();
    assert!(fetched(&repos) > 0, "no corpus repos fetched");

    for repo in &repos {
        if repo.min_endpoints == 0 {
            continue;
        }
        let Some((mut state, _)) = index(repo) else {
            continue;
        };
        let out = state
            .dispatch("list_endpoints", &json!({ "limit": 200 }))
            .unwrap_or_else(|e| panic!("{}: list_endpoints failed: {e:#}", repo.name));
        let total = out["total_detected"].as_u64().unwrap_or(0);
        eprintln!("{:22} endpoints={total}", repo.name);
        assert!(
            total >= repo.min_endpoints,
            "{}: detected {total} endpoints, floor is {}",
            repo.name,
            repo.min_endpoints
        );

        // Paths should look like paths, and a handler should usually be
        // resolvable — a route table full of unlinked entries means the
        // handler association broke.
        let rows = out["endpoints"].as_array().cloned().unwrap_or_default();
        assert!(!rows.is_empty(), "{}: no endpoint rows returned", repo.name);
        let with_handler = rows
            .iter()
            .filter(|e| e.get("handler").is_some_and(|h| !h.is_null()))
            .count();
        eprintln!(
            "{:22} {with_handler}/{} endpoints linked to a handler",
            "",
            rows.len()
        );
    }
}

/// The analysis tools must survive real code without panicking or erroring —
/// scale, odd dialects, generated files and all.
#[test]
#[ignore = "needs scripts/fetch-corpus.sh"]
fn analysis_tools_survive_real_code() {
    let repos = manifest();
    assert!(fetched(&repos) > 0, "no corpus repos fetched");

    for repo in &repos {
        let Some((mut state, _)) = index(repo) else {
            continue;
        };
        for (tool, args) in [
            ("unreferenced_functions", json!({ "limit": 20 })),
            ("list_packages", json!({})),
            ("list_client_calls", json!({ "limit": 20 })),
            ("unreferenced_endpoints", json!({ "limit": 20 })),
            (
                "search_functions",
                json!({ "query": "create", "limit": 10 }),
            ),
        ] {
            state
                .dispatch(tool, &args)
                .unwrap_or_else(|e| panic!("{}: {tool} failed: {e:#}", repo.name));
        }

        // Blast radius from a real function, whichever one search finds first.
        let hits = state
            .dispatch("search_functions", &json!({ "query": "get", "limit": 1 }))
            .expect("search");
        if let Some(name) = hits["functions"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|f| f["name"].as_str())
        {
            let name = name.to_string();
            for tool in [
                "blast_radius",
                "affected_tests",
                "get_callers",
                "get_callees",
            ] {
                state
                    .dispatch(tool, &json!({ "function": name, "limit": 20 }))
                    .unwrap_or_else(|e| panic!("{}: {tool}({name}) failed: {e:#}", repo.name));
            }
        }
        eprintln!("{:22} analysis tools OK", repo.name);
    }
}
