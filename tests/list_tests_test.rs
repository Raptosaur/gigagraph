//! `list_tests` over the `tests/fixtures/apps` mini-apps: every supported
//! language's test idiom must be discoverable, with the right runner, suite
//! and kind.
//!
//! The fixtures deliberately mix declaration-style suites (JUnit, pytest,
//! Go, XCTest), annotation-only ones (xUnit, NUnit, Swift Testing, PHP 8
//! attributes) and block-style ones (Jest/Vitest, RSpec, Catch2, bats) so a
//! regression in any one detection shape fails here.

use gigagraph::api::AppState;
use serde_json::{Value, json};
use std::path::Path;

fn run(args: Value) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apps");
    let mut state = AppState::new(root);
    state
        .dispatch("list_tests", &args)
        .expect("list_tests failed")
}

/// (file, name, framework, kind, suite) for every returned row.
fn rows(out: &Value) -> Vec<(String, String, String, String, String)> {
    out["files"]
        .as_array()
        .expect("files")
        .iter()
        .flat_map(|f| {
            let path = f["file"].as_str().unwrap().to_string();
            f["tests"].as_array().unwrap().iter().map(move |t| {
                (
                    path.clone(),
                    t["name"].as_str().unwrap().to_string(),
                    t["framework"].as_str().unwrap().to_string(),
                    t["kind"].as_str().unwrap().to_string(),
                    t["suite"].as_str().unwrap_or("").to_string(),
                )
            })
        })
        .collect()
}

fn has(rows: &[(String, String, String, String, String)], name: &str, framework: &str) -> bool {
    rows.iter().any(|r| r.1 == name && r.2 == framework)
}

fn names_in(rows: &[(String, String, String, String, String)], file_part: &str) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .filter(|r| r.0.contains(file_part))
        .map(|r| r.1.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn finds_cases_in_every_supported_language() {
    let out = run(json!({ "limit": 500 }));
    let rows = rows(&out);

    // One assertion per runner: name + framework must both be right, so a
    // case attributed to the wrong framework fails as loudly as a missing one.
    for (name, framework) in [
        // Rust
        ("parses_counter_lines", "rust-test"),
        ("runs_end_to_end", "rust-test"),
        // Go — plain, benchmark and fuzz targets all count
        ("TestReserveReducesAvailability", "go-test"),
        ("BenchmarkReserve", "go-test"),
        ("FuzzReserve", "go-test"),
        // Python
        ("test_addition_sums_cents", "pytest"),
        ("test_parse_reads_decimals", "pytest"),
        ("test_unknown_account_is_created", "unittest"),
        // JS/TS block style
        ("sums line totals", "jest"),
        ("renders a book view", "vitest"),
        ("warms the cache", "vitest"),
        // `it.each([...])("...")` — the title lives in the curried call
        ("formats %i cents", "vitest"),
        // Java / Kotlin
        ("admitStoresThePatient", "junit"),
        ("minorsAreUnderEighteen", "junit"),
        ("`total sums every charge`", "junit"),
        ("employerCopayIsAlwaysZero", "junit"),
        // C#
        ("SubtotalSumsScannedItems", "xunit"),
        ("TotalAppliesTax", "xunit"),
        ("RoundsToCents", "nunit"),
        ("ScalesLinearly", "nunit"),
        // PHP — camelCase convention and the PHP 8 #[Test] attribute
        ("testOpenReturnsASlug", "phpunit"),
        ("itListsRecentThreads", "phpunit"),
        // Ruby
        ("blocks banned words", "rspec"),
        ("test_visible_hides_blocked_threads", "minitest"),
        // C / C++
        ("test_pid_clamps_high", "c-test"),
        ("DistanceToSelfIsZero", "gtest"),
        ("AppendGrowsTheRoute", "gtest"),
        ("haversine is symmetric", "catch2"),
        // Swift / Objective-C
        ("testRefreshAppendsAReading", "xctest"),
        ("testPendingCountStartsAtZero", "xctest"),
        ("lowBatteryThreshold", "swift-testing"),
        // Bash
        ("retry gives up after the attempt budget", "bats"),
        ("testLogInfoWritesToStderr", "shunit2"),
    ] {
        assert!(
            has(&rows, name, framework),
            "missing {framework} case `{name}`"
        );
    }

    // Every framework the fixtures exercise should appear in the summary.
    let frameworks = out["frameworks"].as_object().expect("frameworks");
    for f in [
        "rust-test",
        "go-test",
        "pytest",
        "unittest",
        "jest",
        "vitest",
        "junit",
        "xunit",
        "nunit",
        "phpunit",
        "rspec",
        "minitest",
        "c-test",
        "gtest",
        "catch2",
        "xctest",
        "swift-testing",
        "bats",
        "shunit2",
    ] {
        assert!(frameworks.contains_key(f), "framework {f} not summarised");
    }
}

#[test]
fn reports_suites_and_scopes_cases_to_them() {
    let out = run(json!({ "limit": 500 }));
    let rows = rows(&out);

    // Class-based suites come from containing_type.
    let junit = rows
        .iter()
        .find(|r| r.1 == "admitStoresThePatient")
        .expect("junit case");
    assert_eq!(junit.4, "PatientServiceTest");

    // gtest's suite is parsed out of `TEST(Suite, Case)`.
    let gtest = rows
        .iter()
        .find(|r| r.1 == "EmptyRouteHasZeroLength")
        .expect("gtest case");
    assert_eq!(gtest.4, "RouteTest");

    // Nested `describe` blocks resolve to the INNERMOST enclosing group.
    let nested = rows
        .iter()
        .find(|r| r.1 == "exposes its ttl")
        .expect("nested vitest case");
    assert_eq!(nested.4, "CatalogService");
    let outer = rows
        .iter()
        .find(|r| r.1 == "renders a book view")
        .expect("outer vitest case");
    assert_eq!(outer.4, "catalog");

    // RSpec example groups nest the same way.
    let rspec = rows
        .iter()
        .find(|r| r.1 == "matches case-insensitively")
        .expect("rspec case");
    assert_eq!(rspec.4, "with mixed case");
}

#[test]
fn excludes_scaffolding_unless_asked() {
    let cases = run(json!({ "limit": 500 }));
    let case_rows = rows(&cases);
    assert!(
        case_rows.iter().all(|r| r.3 == "case"),
        "default result should be cases only"
    );
    for scaffolding in [
        "setUp",
        "tearDown",
        "journal",
        "usd",
        "newFixture",
        "helper_amount",
    ] {
        assert!(
            !case_rows.iter().any(|r| r.1 == scaffolding),
            "`{scaffolding}` is scaffolding, not a case"
        );
    }

    let all = run(json!({ "limit": 500, "include_hooks": true }));
    let all_rows = rows(&all);
    assert!(all_rows.len() > case_rows.len(), "hooks add nothing");
    // pytest fixtures, JUnit @BeforeEach and xUnit-style setUp all surface as
    // hooks; describe blocks as suites.
    assert!(
        all_rows
            .iter()
            .any(|r| r.1 == "journal" && r.2 == "pytest" && r.3 == "hook"),
        "pytest fixture missing from hooks"
    );
    assert!(
        all_rows
            .iter()
            .any(|r| r.1 == "setUp" && r.2 == "junit" && r.3 == "hook"),
        "@BeforeEach missing from hooks"
    );
    assert!(
        all_rows.iter().any(|r| r.1 == "catalog" && r.3 == "suite"),
        "describe block missing from suites"
    );
    assert_eq!(
        cases["total_cases"], all["total_cases"],
        "total_cases counts cases regardless of include_hooks"
    );
}

#[test]
fn does_not_mistake_production_code_for_tests() {
    let out = run(json!({ "limit": 500, "include_hooks": true }));
    let rows = rows(&out);
    // Names that look testish but are production code, and helpers that live
    // in test files but are not cases.
    for not_a_test in [
        "toView",      // bookstore catalog
        "restock",     // arrow export
        "TaxFor",      // C# interface member
        "describe",    // Kotlin `Slot.describe()` extension
        "reply_count", // PHP free function
        "makeReading", // Swift test helper
        "helperRoute", // gtest file helper
        "resetFixtures",
        "build_thread",
        "require_cmd", // bash library function
        "log_info",
    ] {
        assert!(
            !rows.iter().any(|r| r.1 == not_a_test),
            "`{not_a_test}` wrongly reported as a test"
        );
    }
}

#[test]
fn filters_narrow_the_inventory() {
    let all = run(json!({ "limit": 500 }));
    let total = all["matched"].as_u64().unwrap();

    let go = run(json!({ "language": "go" }));
    let go_rows = rows(&go);
    assert!(!go_rows.is_empty());
    assert!(go_rows.iter().all(|r| r.0.ends_with(".go")));
    assert!(go["matched"].as_u64().unwrap() < total);

    let pytest = run(json!({ "framework": "pytest" }));
    assert!(rows(&pytest).iter().all(|r| r.2 == "pytest"));

    let by_file = run(json!({ "file": "ledger/tests/test_money.py" }));
    assert_eq!(
        names_in(&rows(&by_file), "test_money.py").len(),
        6,
        "test_money.py has six cases"
    );

    // `name` matches the case name or its suite.
    let by_name = run(json!({ "name": "copay" }));
    let named = rows(&by_name);
    assert!(!named.is_empty());
    assert!(named.iter().all(|r| r.1.to_lowercase().contains("copay")));

    let by_suite = run(json!({ "name": "CatalogService" }));
    assert!(
        rows(&by_suite).iter().any(|r| r.1 == "exposes its ttl"),
        "suite name should match"
    );

    // Unknown filters return nothing rather than everything.
    assert_eq!(run(json!({ "framework": "nosuchrunner" }))["matched"], 0);
}

#[test]
fn truncates_without_losing_the_totals() {
    let out = run(json!({ "limit": 3 }));
    assert_eq!(rows(&out).len(), 3);
    assert_eq!(out["truncated"], true);
    // The counts describe the whole repo, not the page.
    assert!(out["matched"].as_u64().unwrap() > 3);
    assert!(out["total_cases"].as_u64().unwrap() > 3);
}
