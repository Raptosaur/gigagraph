//! Exhaustive extraction coverage over the `tests/fixtures/apps` mini-apps.
//!
//! Every other language test spot-checks ("did we get `foo`?"). These assert
//! SET EQUALITY between the functions a file defines and the functions the
//! extractor reports, so a regression that silently stops extracting a
//! construct fails here even though every spot-check still passes. The
//! fixtures are eight small but realistic applications spanning all twenty
//! supported languages, each with its own idiomatic test suite.
//!
//! Known, deliberate exclusions from "every function":
//! - Declarations without a body (C/C++ prototypes, Kotlin/Swift protocol
//!   requirements that DO have a default body are included; bare ones are
//!   listed where the grammar reports them).
//! - The synthetic `(toplevel)` holder, filtered by `function_names`.
//! - Properties that are not functions (C# `=>` properties over a field,
//!   Swift computed `var`s).
//! See `known_gaps` at the bottom for constructs the extractor genuinely
//! cannot see yet — asserted as absent so the day they start working is a
//! visible, deliberate change.

mod common;

use common::*;

fn app(rel: &str) -> String {
    format!("apps/{rel}")
}

// ---------------------------------------------------------------------------
// bookstore — TypeScript / TSX / JavaScript / Prisma / SQL / GraphQL
// ---------------------------------------------------------------------------

#[test]
fn bookstore_typescript_sources() {
    assert_exact_functions(
        &extract_fixture("ts", &app("bookstore/src/catalog.ts")),
        &[
            "findBook",
            "searchBooks",
            "toView",
            "constructor",
            "warm",
            "ttl",
            // `export const restock = async (...) => {}`
            "restock",
        ],
    );

    assert_exact_functions(
        &extract_fixture("ts", &app("bookstore/src/format.ts")),
        &["formatPrice", "slugify", "titleCase"],
    );

    assert_exact_functions(
        &extract_fixture("ts", &app("bookstore/src/cart.ts")),
        &[
            "add",
            "remove", // `get total()` is a real accessor method
            "total",  // `static empty()`
            "empty",
            "addToCart",
        ],
    );

    // server.ts is route wiring: the express handlers are anonymous arrows
    // passed to `app.get(...)`, so `start` is the only named function.
    assert_exact_functions(
        &extract_fixture("ts", &app("bookstore/src/server.ts")),
        &["start"],
    );
}

#[test]
fn bookstore_react_and_javascript() {
    // `handleCheckout` is a `useCallback`-wrapped arrow — a named function to
    // every human reading the file, so the graph must see it too.
    assert_exact_functions(
        &extract_fixture("tsx", &app("bookstore/web/Cart.tsx")),
        &["CartPanel", "handleCheckout", "EmptyCart", "CartPage"],
    );

    assert_exact_functions(
        &extract_fixture("js", &app("bookstore/web/analytics.js")),
        &["trackEvent", "flush", "identify"],
    );
}

#[test]
fn bookstore_javascript_test_suites() {
    // BDD suites declare cases as calls; only the helper is a definition.
    assert_exact_functions(
        &extract_fixture("ts", &app("bookstore/tests/catalog.test.ts")),
        &["resetFixtures"],
    );
    assert_exact_functions(
        &extract_fixture("js", &app("bookstore/tests/cart.spec.js")),
        &[],
    );
}

#[test]
fn bookstore_schema_languages() {
    // Schema languages map CREATE'd/declared objects onto "functions".
    let sql = extract_fixture("sql", &app("bookstore/db/schema.sql"));
    assert_exact_functions(&sql, &["authors", "books", "book_catalog", "cart_total"]);

    assert_exact_functions(
        &extract_fixture("prisma", &app("bookstore/prisma/schema.prisma")),
        &["Book", "Author", "Cart", "CartLine"],
    );

    assert_exact_functions(
        &extract_fixture("graphql", &app("bookstore/api/schema.graphql")),
        &["Author", "Book", "Query", "Mutation"],
    );
}

// ---------------------------------------------------------------------------
// warehouse — Go / Bash / JavaScript (Lambda handlers)
// ---------------------------------------------------------------------------

#[test]
fn warehouse_go_sources() {
    assert_exact_functions(
        &extract_fixture("go", &app("warehouse/internal/inventory/inventory.go")),
        &[
            "Available",
            "String",
            "NewService",
            "Put",
            "Reserve",
            "Release",
            "max",
        ],
    );

    assert_exact_functions(
        &extract_fixture("go", &app("warehouse/internal/store/store.go")),
        &["Open", "Load", "Save", "Close"],
    );

    assert_exact_functions(
        &extract_fixture("go", &app("warehouse/cmd/warehoused/main.go")),
        &[
            "main",
            "handleHealth",
            "handleGetItem",
            "handleReserve",
            "writeJSON",
        ],
    );

    assert_exact_functions(
        &extract_fixture("go", &app("warehouse/internal/inventory/inventory_test.go")),
        &[
            "TestReserveReducesAvailability",
            "TestReserveRejectsOverdraft",
            "TestReleaseIsIdempotent",
            "BenchmarkReserve",
            "FuzzReserve",
            "newFixture",
        ],
    );
}

#[test]
fn warehouse_bash_sources() {
    assert_exact_functions(
        &extract_fixture("sh", &app("warehouse/scripts/deploy.sh")),
        &["build_image", "push_image", "rollout", "main"],
    );

    assert_exact_functions(
        &extract_fixture("sh", &app("warehouse/scripts/lib.sh")),
        &["log_info", "log_error", "require_cmd", "retry"],
    );

    assert_exact_functions(
        &extract_fixture("sh", &app("warehouse/test/lib_shunit.sh")),
        &[
            "testLogInfoWritesToStderr",
            "testLogErrorWritesToStderr",
            "oneTimeSetUp",
        ],
    );

    // Bats: `@test "..." { }` is not bash syntax, so the case is recovered
    // from the header command and named after its description.
    assert_exact_functions(
        &extract_fixture("bats", &app("warehouse/test/deploy.bats")),
        &[
            "setup",
            "require_cmd succeeds for an existing command",
            "require_cmd fails for a missing command",
            "retry gives up after the attempt budget",
            "teardown",
        ],
    );
}

#[test]
fn warehouse_lambda_handlers() {
    assert_exact_functions(
        &extract_fixture("js", &app("warehouse/jobs/reindex.js")),
        &["handler", "warm"],
    );
    assert_exact_functions(
        &extract_fixture("js", &app("warehouse/jobs/count.js")),
        &["handler"],
    );
    assert_exact_functions(
        &extract_fixture("js", &app("warehouse/jobs/indexer.js")),
        &["rebuild", "chunk"],
    );
}

// ---------------------------------------------------------------------------
// ledger — Python
// ---------------------------------------------------------------------------

#[test]
fn ledger_python_sources() {
    assert_exact_functions(
        &extract_fixture("py", &app("ledger/ledger/__init__.py")),
        &["version"],
    );

    assert_exact_functions(
        &extract_fixture("py", &app("ledger/ledger/money.py")),
        &[
            "__add__",
            "__neg__",
            "as_decimal",
            // @classmethod / @staticmethod / @property all stay functions
            "zero",
            "parse",
            "is_zero",
            "_assert_same_currency",
            "total",
        ],
    );

    assert_exact_functions(
        &extract_fixture("py", &app("ledger/ledger/accounts.py")),
        &[
            // Account
            "__init__",
            "post",
            "balance",
            "__repr__",
            // Journal — a second __init__ on a different class counts twice
            "__init__",
            "account",
            "transfer",
            "balances",
            "snapshot",
            "chart_of_accounts",
            "is_balanced",
        ],
    );

    assert_exact_functions(
        &extract_fixture("py", &app("ledger/ledger/api.py")),
        &["health", "read_balance", "create_transfer", "list_balances"],
    );
}

#[test]
fn ledger_python_tests() {
    assert_exact_functions(
        &extract_fixture("py", &app("ledger/tests/conftest.py")),
        &["journal", "usd"],
    );

    assert_exact_functions(
        &extract_fixture("py", &app("ledger/tests/test_money.py")),
        &[
            "test_addition_sums_cents",
            "test_addition_rejects_currency_mismatch",
            "test_parse_reads_decimals",
            "test_total_of_empty_is_zero",
            "test_zero_is_zero",
            "test_negation_flips_sign",
            "helper_amount",
        ],
    );

    assert_exact_functions(
        &extract_fixture("py", &app("ledger/tests/test_accounts.py")),
        &[
            "test_transfer_balances_out",
            "test_chart_of_accounts_filters_by_kind",
            "setUp",
            "test_unknown_account_is_created",
            "test_balances_lists_every_account",
            "tearDown",
        ],
    );
}

// ---------------------------------------------------------------------------
// telemetry — Rust
// ---------------------------------------------------------------------------

#[test]
fn telemetry_rust_sources() {
    assert_exact_functions(
        &extract_fixture("rs", &app("telemetry/src/lib.rs")),
        &["version"],
    );

    assert_exact_functions(
        &extract_fixture("rs", &app("telemetry/src/metrics.rs")),
        &[
            // impl Display
            "fmt", // impl Registry
            "new",
            "incr",
            "set_gauge",
            "get",
            "render",
            // trait Sink: only the defaulted method has a body
            "emit_all", // impl Sink for StdoutSink
            "emit",
            "parse_line",
        ],
    );

    assert_exact_functions(
        &extract_fixture("rs", &app("telemetry/src/pipeline.rs")),
        &[
            "new",
            "ingest",
            "flush",
            "dropped",
            "run",
            // #[cfg(test)] mod tests
            "ingests_counters",
            "counts_dropped_lines",
            "fixture",
        ],
    );

    assert_exact_functions(
        &extract_fixture("rs", &app("telemetry/tests/pipeline_test.rs")),
        &[
            "parses_counter_lines",
            "parses_gauge_lines",
            "rejects_malformed_lines",
            "runs_end_to_end",
            "panics_on_missing_metric",
            "make_pipeline",
        ],
    );
}

// ---------------------------------------------------------------------------
// clinic — Java / Kotlin
// ---------------------------------------------------------------------------

#[test]
fn clinic_java_sources() {
    assert_exact_functions(
        &extract_fixture(
            "java",
            &app("clinic/src/main/java/com/clinic/PatientController.java"),
        ),
        &["PatientController", "list", "byId", "create", "discharge"],
    );

    assert_exact_functions(
        &extract_fixture(
            "java",
            &app("clinic/src/main/java/com/clinic/PatientService.java"),
        ),
        &[
            "PatientService",
            "page",
            "find",
            "admit",
            "discharge",
            "describe",
        ],
    );

    // Record: the compact components are not methods; the declared ones are.
    assert_exact_functions(
        &extract_fixture("java", &app("clinic/src/main/java/com/clinic/Patient.java")),
        &["isMinor", "of"],
    );

    assert_exact_functions(
        &extract_fixture(
            "java",
            &app("clinic/src/test/java/com/clinic/PatientServiceTest.java"),
        ),
        &[
            "setUp",
            "admitStoresThePatient",
            "dischargeRemovesById",
            "minorsAreUnderEighteen",
            "InMemorySchedulerKt",
        ],
    );
}

#[test]
fn clinic_kotlin_sources() {
    assert_exact_functions(
        &extract_fixture("kt", &app("clinic/src/main/kotlin/com/clinic/Scheduler.kt")),
        &[
            // interface Scheduler: two abstract, one defaulted
            "schedule", "cancel", "pending", // class InMemoryScheduler
            "schedule", "cancel", "pending", "nextGap", // companion object
            "empty",   // extension function
            "describe",
        ],
    );

    assert_exact_functions(
        &extract_fixture("kt", &app("clinic/src/main/kotlin/com/clinic/Billing.kt")),
        &["lookup", "total", "copay", "settle"],
    );

    assert_exact_functions(
        &extract_fixture(
            "kt",
            &app("clinic/src/test/kotlin/com/clinic/BillingTest.kt"),
        ),
        &[
            // Backtick-quoted test names keep their backticks.
            "`total sums every charge`",
            "insurerCopayIsOneFifth",
            "employerCopayIsAlwaysZero",
            "visit",
            "cancelReturnsFalseWhenUnknown",
        ],
    );
}

// ---------------------------------------------------------------------------
// pos — C#
// ---------------------------------------------------------------------------

#[test]
fn pos_csharp_sources() {
    assert_exact_functions(
        &extract_fixture("cs", &app("pos/src/Checkout.cs")),
        &[
            // interface member
            "TaxFor",
            // FlatTaxPolicy: expression-bodied ctor + method
            "FlatTaxPolicy",
            "TaxFor",
            // Checkout
            "Checkout",
            "Scan",
            "Void",
            "Subtotal",
            "Total",
            "Empty",
        ],
    );

    assert_exact_functions(
        &extract_fixture("cs", &app("pos/src/Receipts.cs")),
        &["Render", "RenderAsync", "Line", "Print", "Dispose"],
    );

    // Top-level statements only: minimal-API lambdas are anonymous.
    assert_exact_functions(&extract_fixture("cs", &app("pos/src/Program.cs")), &[]);

    assert_exact_functions(
        &extract_fixture("cs", &app("pos/tests/CheckoutTests.cs")),
        &[
            "SubtotalSumsScannedItems",
            "VoidRemovesEveryMatchingLine",
            "TotalAppliesTax",
            "Fixture",
            "RenderIncludesStoreName",
        ],
    );

    assert_exact_functions(
        &extract_fixture("cs", &app("pos/tests/TaxPolicyTests.cs")),
        &["Setup", "RoundsToCents", "ScalesLinearly"],
    );
}

// ---------------------------------------------------------------------------
// forum — PHP / Ruby
// ---------------------------------------------------------------------------

#[test]
fn forum_php_sources() {
    assert_exact_functions(
        &extract_fixture(
            "php",
            &app("forum/app/Http/Controllers/ThreadController.php"),
        ),
        &["__construct", "index", "show", "store", "destroy"],
    );

    assert_exact_functions(
        &extract_fixture("php", &app("forum/app/Services/ThreadService.php")),
        &[
            "open",
            "bySlug",
            "recent",
            "close",
            "slugify",
            // namespaced free function
            "reply_count",
        ],
    );

    assert_exact_functions(
        &extract_fixture("php", &app("forum/tests/Feature/ThreadServiceTest.php")),
        &[
            "setUp",
            "testOpenReturnsASlug",
            "testCloseReturnsFalseForUnknownSlug",
            "itListsRecentThreads",
            "testSlugifyNormalises",
            "titles",
        ],
    );
}

#[test]
fn forum_ruby_sources() {
    assert_exact_functions(
        &extract_fixture("rb", &app("forum/lib/moderation.rb")),
        &[
            // Moderation::Verdict
            "initialize",
            "allowed?",
            "to_s",
            "allow",
            // module functions
            "review",
            "sanitize",
            // Feed
            "initialize",
            "visible",
            "add",
            "normalise",
        ],
    );

    assert_exact_functions(
        &extract_fixture("rb", &app("forum/spec/feed_test.rb")),
        &[
            "setup",
            "test_visible_hides_blocked_threads",
            "test_add_returns_self_for_chaining",
            "teardown",
        ],
    );

    // RSpec cases are `it "..." do` blocks, not `def`s: the only *function*
    // here is the helper. The cases themselves are covered by list_tests.
    assert_exact_functions(
        &extract_fixture("rb", &app("forum/spec/moderation_spec.rb")),
        &["build_thread"],
    );
}

// ---------------------------------------------------------------------------
// drone — C / C++ / Objective-C / Swift
// ---------------------------------------------------------------------------

#[test]
fn drone_c_sources() {
    // Header: only the `static inline` definition has a body; the three
    // prototypes are declarations.
    assert_exact_functions(
        &extract_fixture("h", &app("drone/firmware/pid.h")),
        &["pid_clamp"],
    );

    assert_exact_functions(
        &extract_fixture("c", &app("drone/firmware/pid.c")),
        &["pid_init", "pid_reset", "pid_step"],
    );

    assert_exact_functions(
        &extract_fixture("c", &app("drone/firmware/flight.c")),
        &[
            "flight_boot",
            "flight_tick",
            "flight_land",
            "flight_mode_name",
            "main",
        ],
    );

    assert_exact_functions(
        &extract_fixture("c", &app("drone/tests/pid_test.c")),
        &[
            "make_pid",
            "test_pid_clamps_high",
            "test_pid_reset_clears_integral",
            "main",
        ],
    );
}

#[test]
fn drone_cpp_sources() {
    // Header: only the inline accessor has a body.
    assert_exact_functions(
        &extract_fixture("hpp", &app("drone/nav/waypoint.hpp")),
        &["name"],
    );

    assert_exact_functions(
        &extract_fixture("cpp", &app("drone/nav/waypoint.cpp")),
        &[
            "distanceTo",
            "describe",
            "Route",
            "append",
            "empty",
            "length",
            "parse",
            "haversine",
        ],
    );

    // gtest macros parse as functions named after the macro; the suite/case
    // pair lives in the signature and is recovered by test discovery.
    assert_exact_functions(
        &extract_fixture("cpp", &app("drone/tests/waypoint_test.cpp")),
        &["TEST", "TEST", "TEST", "SetUp", "TEST_F", "helperRoute"],
    );

    // Catch2 declares cases as calls, not definitions.
    assert_exact_functions(
        &extract_fixture("cpp", &app("drone/tests/haversine_test.cpp")),
        &[],
    );
}

#[test]
fn drone_objc_and_swift_sources() {
    assert_exact_functions(
        &extract_fixture("m", &app("drone/ios/Sources/TelemetryBridge.m")),
        &[
            "requiresMainQueueSetup",
            "init",
            "reportAltitude",
            "enqueue",
            "pendingCount",
            "drain",
        ],
    );

    assert_exact_functions(
        &extract_fixture("m", &app("drone/ios/Tests/TelemetryBridgeTests.m")),
        &[
            "setUp",
            "tearDown",
            "testPendingCountStartsAtZero",
            "testEnqueueIncrementsPendingCount",
            "makeBridge",
        ],
    );

    assert_exact_functions(
        &extract_fixture("swift", &app("drone/ios/Sources/Cockpit.swift")),
        &[
            // struct Reading — `var isLow` is a computed property, not a func
            "scaled",
            // protocol requirements
            "latest",
            "history",
            // final class Cockpit
            "init",
            "refresh",
            "averageAltitude",
            "makeDefault",
            // free function
            "formatAltitude",
        ],
    );

    assert_exact_functions(
        &extract_fixture("swift", &app("drone/ios/Tests/CockpitTests.swift")),
        &[
            "setUp",
            "tearDown",
            "testRefreshAppendsAReading",
            "testAverageAltitudeOfEmptyCockpitIsZero",
            "testScaledMultipliesAltitude",
            "makeReading",
            // StubSource
            "latest",
            "history",
            // EmptySource
            "latest",
            "history",
        ],
    );

    assert_exact_functions(
        &extract_fixture("swift", &app("drone/ios/Tests/ReadingTests.swift")),
        &[
            "scalingIsProportional",
            "lowBatteryThreshold",
            "batteryStaysInRange",
            "makeReading",
        ],
    );
}

// ---------------------------------------------------------------------------
// Known gaps — asserted so they stay visible
// ---------------------------------------------------------------------------

#[test]
fn known_gaps_are_still_gaps() {
    // tree-sitter-sequel has no `create_procedure` node: `CREATE PROCEDURE`
    // parses to an ERROR, so the procedure name is unreachable. Its body's
    // table references still land in the file's top-level function, so the
    // dependency edges survive — only the named object is lost.
    let sql = extract_fixture("sql", &app("bookstore/db/schema.sql"));
    assert!(
        !function_names(&sql).contains(&"prune_empty_carts"),
        "CREATE PROCEDURE became extractable — update lang/sql.rs docs and this test"
    );

    // Bats bodies are siblings of the header command in the bash grammar, so
    // a case spans its header line only and the body's calls belong to the
    // file, not the case.
    let bats = extract_fixture("bats", &app("warehouse/test/deploy.bats"));
    let case = func(&bats, "retry gives up after the attempt budget");
    assert_eq!(
        case.start_line, case.end_line,
        "bats cases gained a body span — update lang/bash.rs docs and this test"
    );
    assert!(
        case.calls.is_empty(),
        "bats case bodies became attributable; calls: {:?}",
        case.calls.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
}
