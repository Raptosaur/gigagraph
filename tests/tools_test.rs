//! The index-light inspection tools and `test_command`, exercised over the
//! `tests/fixtures/apps` mini-apps.
//!
//! These exist so an agent can answer three questions without leaving the
//! server: "what did the parser actually see" (`extract_file`), "can this
//! server even read that file" (`supported_languages`), and "how do I run
//! this test" (`test_command`).

use gigagraph::api::AppState;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn apps_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apps")
}

fn run(tool: &str, args: Value) -> Value {
    let mut state = AppState::new(apps_root());
    state
        .dispatch(tool, &args)
        .unwrap_or_else(|e| panic!("{tool} failed: {e:#}"))
}

fn err(tool: &str, args: Value) -> String {
    let mut state = AppState::new(apps_root());
    match state.dispatch(tool, &args) {
        Ok(v) => panic!("{tool} unexpectedly succeeded: {v}"),
        Err(e) => format!("{e:#}"),
    }
}

fn names(v: &Value) -> Vec<&str> {
    v["functions"]
        .as_array()
        .expect("functions")
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// extract_file
// ---------------------------------------------------------------------------

#[test]
fn extract_file_shows_what_the_parser_saw() {
    let out = run("extract_file", json!({ "path": "ledger/ledger/money.py" }));
    assert_eq!(out["language"], "python");
    let names = names(&out);
    assert!(names.contains(&"as_decimal"), "{names:?}");
    assert!(names.contains(&"_assert_same_currency"), "{names:?}");

    // Decorations are the whole point: they are what test/endpoint detection
    // keys off, and the resolved graph only keeps a boolean.
    let zero = out["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "zero")
        .expect("zero");
    assert_eq!(zero["decorations"][0], "classmethod");
    assert_eq!(zero["containing_type"], "Money");
}

#[test]
fn extract_file_keeps_literal_call_arguments() {
    // The route path in a decorator and the case name in `it("...")` are both
    // literals that CallSite discards after resolution.
    let api = run("extract_file", json!({ "path": "ledger/ledger/api.py" }));
    let health = api["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "health")
        .expect("health");
    assert_eq!(health["decorations"][0], "app.get");

    let spec = run(
        "extract_file",
        json!({ "path": "bookstore/tests/catalog.test.ts" }),
    );
    let literals: Vec<String> = spec["functions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["literal_calls"].as_array().cloned().unwrap_or_default())
        .flat_map(|c| c["literals"].as_array().cloned().unwrap_or_default())
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    assert!(
        literals.iter().any(|l| l == "renders a book view"),
        "case names should survive: {literals:?}"
    );
}

#[test]
fn extract_file_reports_hierarchy_and_type_decorations() {
    let out = run(
        "extract_file",
        json!({ "path": "forum/tests/Feature/ThreadServiceTest.php" }),
    );
    let bases: Vec<&str> = out["hierarchy"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["base"].as_str())
        .collect();
    assert!(bases.contains(&"TestCase"), "{bases:?}");
}

#[test]
fn extract_file_explains_an_unreadable_extension() {
    // Markdown has no LangSpec, so the file is invisible to the index; the
    // error must route to the tool that explains why rather than leaving the
    // caller to guess.
    let readme = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let msg = err("extract_file", json!({ "path": readme.to_string_lossy() }));
    assert!(
        msg.contains("supported_languages"),
        "error should route to supported_languages: {msg}"
    );
}

// ---------------------------------------------------------------------------
// supported_languages
// ---------------------------------------------------------------------------

#[test]
fn supported_languages_lists_every_registered_language() {
    let out = run("supported_languages", json!({}));
    let langs: Vec<&str> = out["languages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["language"].as_str())
        .collect();
    for lang in [
        "bash",
        "c",
        "cpp",
        "csharp",
        "go",
        "graphql",
        "java",
        "javascript",
        "kotlin",
        "objc",
        "php",
        "prisma",
        "python",
        "ruby",
        "rust",
        "sql",
        "swift",
        "typescript",
    ] {
        assert!(langs.contains(&lang), "{lang} missing from {langs:?}");
    }
}

#[test]
fn supported_languages_answers_about_one_path() {
    let bats = run("supported_languages", json!({ "path": "test/deploy.bats" }));
    assert_eq!(bats["indexable"], true);
    assert_eq!(bats["path_language"], "bash");

    // YAML IS registered (shallow scan for IaC routes) — the honest negative
    // is a format with no parser at all.
    let yaml = run(
        "supported_languages",
        json!({ "path": "docker-compose.yml" }),
    );
    assert_eq!(yaml["indexable"], true);
    assert_eq!(yaml["path_language"], "yaml");

    let md = run("supported_languages", json!({ "path": "README.md" }));
    assert_eq!(md["indexable"], false);
    assert!(md["path_language"].is_null());
}

// ---------------------------------------------------------------------------
// file_overview over a directory
// ---------------------------------------------------------------------------

#[test]
fn file_overview_covers_a_whole_directory() {
    let out = run("file_overview", json!({ "dir": "ledger/ledger" }));
    let files: Vec<&str> = out["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["path"].as_str())
        .collect();
    assert_eq!(files.len(), 4, "{files:?}");
    assert!(files.iter().all(|p| p.starts_with("ledger/ledger/")));
    assert_eq!(out["truncated"], false);

    // A prefix must not match a sibling by string prefix alone.
    let single = run("file_overview", json!({ "dir": "ledger/ledger/api.py" }));
    assert_eq!(single["total_files"], 1);
}

#[test]
fn file_overview_truncates_and_still_reports_the_total() {
    let out = run("file_overview", json!({ "dir": "ledger", "limit": 2 }));
    assert_eq!(out["files"].as_array().unwrap().len(), 2);
    assert_eq!(out["truncated"], true);
    assert!(out["total_files"].as_u64().unwrap() > 2);
}

#[test]
fn file_overview_still_takes_a_single_path() {
    let out = run("file_overview", json!({ "path": "ledger/ledger/money.py" }));
    assert_eq!(out["path"], "ledger/ledger/money.py");
    assert!(names(&out).contains(&"total"));
}

// ---------------------------------------------------------------------------
// test_command
// ---------------------------------------------------------------------------

fn command_for(args: Value) -> String {
    let out = run("test_command", args);
    out["commands"][0]["command"]
        .as_str()
        .expect("command")
        .to_string()
}

#[test]
fn test_command_speaks_each_runner() {
    for (args, expected) in [
        (
            json!({ "file": "test_money.py", "name": "test_total_of_empty_is_zero" }),
            "pytest ledger/tests/test_money.py -k 'test_total_of_empty_is_zero'",
        ),
        (
            json!({ "file": "catalog.test.ts", "name": "slugifies titles" }),
            "npx vitest run bookstore/tests/catalog.test.ts -t 'slugifies titles'",
        ),
        (
            json!({ "file": "cart.spec.js", "name": "removes a line" }),
            "npx jest bookstore/tests/cart.spec.js -t 'removes a line'",
        ),
        (
            json!({ "file": "inventory_test.go", "name": "FuzzReserve" }),
            "go test -run '^FuzzReserve$' ./warehouse/internal/inventory",
        ),
        (
            json!({ "file": "deploy.bats", "name": "retry gives up after the attempt budget" }),
            "bats warehouse/test/deploy.bats -f 'retry gives up after the attempt budget'",
        ),
        (
            json!({ "file": "CheckoutTests.cs", "name": "TotalAppliesTax" }),
            "dotnet test --filter 'FullyQualifiedName~CheckoutTests.TotalAppliesTax'",
        ),
        (
            json!({ "file": "waypoint_test.cpp", "name": "DistanceToSelfIsZero" }),
            "<test-binary> --gtest_filter=WaypointTest.DistanceToSelfIsZero",
        ),
        (
            json!({ "file": "ThreadServiceTest.php", "name": "itListsRecentThreads" }),
            "phpunit --filter 'itListsRecentThreads' forum/tests/Feature/ThreadServiceTest.php",
        ),
        (
            json!({ "file": "feed_test.rb", "name": "test_add_returns_self_for_chaining" }),
            "ruby -Itest forum/spec/feed_test.rb -n test_add_returns_self_for_chaining",
        ),
        (
            json!({ "file": "lib_shunit.sh" }),
            "sh warehouse/test/lib_shunit.sh",
        ),
    ] {
        assert_eq!(command_for(args.clone()), expected, "for {args}");
    }
}

#[test]
fn test_command_follows_the_projects_build_tooling() {
    // No pom.xml or build.gradle in the fixture tree, so JUnit falls back to
    // plain maven rather than inventing a wrapper.
    let junit = command_for(json!({ "file": "BillingTest.kt", "name": "insurerCopayIsOneFifth" }));
    assert_eq!(junit, "mvn test -Dtest=BillingTest#insurerCopayIsOneFifth");

    // No Package.swift either, so XCTest gets the xcodebuild form with an
    // explicit placeholder rather than a command that would silently fail.
    let xctest =
        command_for(json!({ "file": "CockpitTests.swift", "name": "testRefreshAppendsAReading" }));
    assert!(
        xctest.starts_with("xcodebuild test -only-testing:"),
        "{xctest}"
    );
    assert!(
        xctest.contains("CockpitTests/testRefreshAppendsAReading"),
        "{xctest}"
    );
}

#[test]
fn test_command_addresses_rspec_cases_by_line() {
    let out = run(
        "test_command",
        json!({ "file": "moderation_spec.rb", "name": "strips tags" }),
    );
    let cmd = out["commands"][0]["command"].as_str().unwrap();
    // Renaming an RSpec example breaks a name filter; the line does not.
    assert!(
        cmd.starts_with("rspec forum/spec/moderation_spec.rb:"),
        "{cmd}"
    );
}

#[test]
fn test_command_always_offers_the_whole_file() {
    let out = run(
        "test_command",
        json!({ "file": "test_money.py", "name": "test_total_of_empty_is_zero" }),
    );
    assert_eq!(
        out["commands"][0]["file_command"],
        "pytest ledger/tests/test_money.py"
    );
}

#[test]
fn test_command_refuses_to_guess() {
    assert!(err("test_command", json!({})).contains("file"));
    let msg = err("test_command", json!({ "file": "nope_test.py" }));
    assert!(
        msg.contains("list_tests"),
        "should route to list_tests: {msg}"
    );
}

// ---------------------------------------------------------------------------
// index_stats
// ---------------------------------------------------------------------------

#[test]
fn index_stats_reports_coverage_gaps_and_endpoint_links() {
    let out = run("index_stats", json!({}));
    let stats = &out["stats"];
    assert!(stats["files"].as_u64().unwrap() > 0);

    // openapi.yaml and serverless.yml are collected for IaC scanning but have
    // no parser, so they must show up as skipped rather than vanish silently.
    let skipped: Vec<&str> = stats["skipped_paths"]
        .as_array()
        .expect("skipped_paths")
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    assert_eq!(
        skipped.len() as u64,
        stats["skipped_files"].as_u64().unwrap().min(50),
        "sample should match the count until the cap: {skipped:?}"
    );

    // Handler links are a route-detection health metric, not just a count.
    let linked = stats["endpoints_with_handler"].as_u64().unwrap();
    assert!(
        linked > 0,
        "fixtures publish routes with resolvable handlers"
    );
}
