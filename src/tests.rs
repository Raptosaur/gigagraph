//! Test discovery across every indexed language.
//!
//! `is_test` on `FunctionInfo` answers a coarse question — "would changing
//! this dirty a test run?" — and deliberately over-includes: every helper in a
//! test file counts, because the file is the re-run unit. This module answers
//! the sharper question `list_tests` needs: *which named test cases exist,
//! under which framework, in which suite*. The two disagree on purpose —
//! `setUp`, fixtures and helpers are `is_test` but are not cases.
//!
//! Three detection shapes cover every supported language:
//!
//! 1. **Decorations** — `#[test]`, `@Test`, `[Fact]`, `@pytest.mark.*`,
//!    `#[Test]` (PHP 8 attributes), Swift Testing's `@Test`. Unambiguous:
//!    the annotation exists to mark a test.
//! 2. **Naming conventions** — Go `TestXxx`/`BenchmarkXxx`/`FuzzXxx`, Python
//!    and Ruby `test_*`, PHPUnit/XCTest/shunit2 `testXxx`. Conventions that a
//!    runner actually enforces, so a match is not a guess. Language-scoped:
//!    `testConnection()` in a Go non-test file is not a case.
//! 3. **Block calls** — the BDD families, where the "test" is a call with a
//!    string-literal name rather than a declaration: JS/TS
//!    `describe`/`it`/`test` (plus `.only`/`.skip`/`.each` modifiers), RSpec
//!    `describe`/`context`/`it`, Catch2 `TEST_CASE`/`SCENARIO`. These need the
//!    raw call literals, which is why detection runs at graph-build time
//!    alongside endpoint detection — `CallSite` does not carry `arg_lits`.
//!
//! gtest is the odd one out: `TEST(Suite, Case) { ... }` parses as a function
//! *named* `TEST`, with the suite and case name sitting in the declarator, so
//! the case name is recovered from the signature text.

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::extract::{LitKind, RawCall};
use crate::types::{FileInfo, FunctionInfo, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestKind {
    /// A single executable test case.
    Case,
    /// A grouping construct: `describe`/`context` block, gtest fixture class,
    /// RSpec example group.
    Suite,
    /// Setup/teardown/fixture scaffolding that runs around cases.
    Hook,
}

impl TestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TestKind::Case => "case",
            TestKind::Suite => "suite",
            TestKind::Hook => "hook",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: u32,
    /// Human-readable case name: the declared name, or the string literal for
    /// block-style frameworks.
    pub name: String,
    /// Detected runner: `pytest`, `jest`, `go-test`, `junit`, `xunit`,
    /// `rspec`, `gtest`, `catch2`, `xctest`, `bats`, ...
    pub framework: String,
    pub kind: TestKind,
    pub file_id: u32,
    pub line: u32,
    /// Enclosing suite: class/`describe` name, gtest suite, RSpec group.
    pub suite: Option<String>,
    /// Indexed function this case is (declaration-style) or lives in
    /// (block-style). `None` only when the owning function is unknown.
    pub function: Option<u32>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TestIndex {
    pub cases: Vec<TestCase>,
}

impl TestIndex {
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// Cases (not suites/hooks) only — the "how many tests are there" count.
    pub fn case_count(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| c.kind == TestKind::Case)
            .count()
    }

    pub fn frameworks(&self) -> Vec<(String, usize)> {
        let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
        for c in &self.cases {
            *counts.entry(c.framework.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }
}

/// Build the test index. `raw_calls` and `decorations` are indexed by function
/// id, exactly as `endpoints::detect` receives them.
pub fn detect(
    files: &[FileInfo],
    functions: &[FunctionInfo],
    raw_calls: &[Vec<RawCall>],
    decorations: &[Vec<crate::extract::RawDecoration>],
    hierarchy: &[Vec<(String, String)>],
) -> TestIndex {
    let mut idx = TestIndex::default();

    // (file id -> derived type -> base types), for XCTestCase/TestCase checks.
    let bases: Vec<FxHashMap<&str, Vec<&str>>> = hierarchy
        .iter()
        .map(|edges| {
            let mut m: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
            for (derived, base) in edges {
                m.entry(derived.as_str()).or_default().push(base.as_str());
            }
            m
        })
        .collect();

    for (fn_id, func) in functions.iter().enumerate() {
        let file = &files[func.file_id as usize];
        let file_bases = bases.get(func.file_id as usize);
        let decos = decorations.get(fn_id).map(Vec::as_slice).unwrap_or(&[]);

        if let Some((name, framework, kind, suite)) =
            classify_function(func, file, decos, file_bases)
        {
            push(
                &mut idx,
                name,
                framework,
                kind,
                func.file_id,
                func.start_line,
                suite,
                Some(fn_id as u32),
            );
        }

        let calls = raw_calls.get(fn_id).map(Vec::as_slice).unwrap_or(&[]);
        collect_block_tests(&mut idx, func, file, calls, fn_id as u32);
    }

    idx.cases.sort_by_key(|c| (c.file_id, c.line, c.id));
    for (i, case) in idx.cases.iter_mut().enumerate() {
        case.id = i as u32;
    }
    idx
}

#[allow(clippy::too_many_arguments)]
fn push(
    idx: &mut TestIndex,
    name: String,
    framework: &str,
    kind: TestKind,
    file_id: u32,
    line: u32,
    suite: Option<String>,
    function: Option<u32>,
) {
    idx.cases.push(TestCase {
        id: idx.cases.len() as u32,
        name,
        framework: framework.to_string(),
        kind,
        file_id,
        line,
        suite,
        function,
    });
}

// ---------------------------------------------------------------------------
// Declaration-style detection
// ---------------------------------------------------------------------------

/// Classify a declared function as a case/suite/hook, if it is one.
/// Returns (display name, framework, kind, suite).
fn classify_function(
    func: &FunctionInfo,
    file: &FileInfo,
    decos: &[crate::extract::RawDecoration],
    bases: Option<&FxHashMap<&str, Vec<&str>>>,
) -> Option<(String, &'static str, TestKind, Option<String>)> {
    let deco_names: Vec<&str> = decos.iter().map(|d| last_segment(&d.name)).collect();
    let suite = func.containing_type.clone();

    // gtest's TEST/TEST_F/TEST_P parse as functions named after the macro; the
    // real suite and case names live in the signature: `TEST(Suite, Case) {`.
    if matches!(func.language, Lang::Cpp | Lang::C)
        && matches!(
            func.name.as_str(),
            "TEST"
                | "TEST_F"
                | "TEST_P"
                | "TYPED_TEST"
                | "TYPED_TEST_P"
                | "INSTANTIATE_TEST_SUITE_P"
        )
    {
        let (gsuite, case) = gtest_names(&func.signature)?;
        return Some((case, "gtest", TestKind::Case, Some(gsuite)));
    }

    // 1. Decoration-driven — unambiguous across ecosystems.
    if let Some((framework, kind)) = framework_for_decorations(&deco_names, func.language) {
        return Some((func.name.clone(), framework, kind, suite));
    }

    // 2. Naming conventions the runner itself enforces.
    let in_test_file = crate::graph::is_test_file(&file.path);
    match func.language {
        Lang::Go => {
            for (prefix, framework) in [
                ("Test", "go-test"),
                ("Benchmark", "go-test"),
                ("Fuzz", "go-test"),
                ("Example", "go-test"),
            ] {
                if starts_upper_after(&func.name, prefix) {
                    return Some((func.name.clone(), framework, TestKind::Case, suite));
                }
            }
        }
        Lang::Python => {
            if func.name.starts_with("test_") {
                let framework = if inherits_any(bases, suite.as_deref(), &["TestCase"]) {
                    "unittest"
                } else {
                    "pytest"
                };
                return Some((func.name.clone(), framework, TestKind::Case, suite));
            }
            if in_test_file
                && matches!(
                    func.name.as_str(),
                    "setUp" | "tearDown" | "setUpClass" | "tearDownClass"
                )
            {
                return Some((func.name.clone(), "unittest", TestKind::Hook, suite));
            }
        }
        Lang::Ruby => {
            if func.name.starts_with("test_") {
                return Some((func.name.clone(), "minitest", TestKind::Case, suite));
            }
            if in_test_file && matches!(func.name.as_str(), "setup" | "teardown") {
                return Some((func.name.clone(), "minitest", TestKind::Hook, suite));
            }
        }
        Lang::Php => {
            // `/** @test */` frees the method from the `testXxx` prefix; the
            // annotation reaches us as a raw comment decoration (lang/php.rs).
            if decos.iter().any(|d| d.name.contains("@test")) {
                return Some((func.name.clone(), "phpunit", TestKind::Case, suite));
            }
            // PHPUnit runs `testXxx` methods in `*Test` classes.
            if starts_upper_after(&func.name, "test")
                && (in_test_file || inherits_any(bases, suite.as_deref(), &["TestCase"]))
            {
                return Some((func.name.clone(), "phpunit", TestKind::Case, suite));
            }
            if in_test_file
                && matches!(
                    func.name.as_str(),
                    "setUp" | "tearDown" | "setUpBeforeClass" | "tearDownAfterClass"
                )
            {
                return Some((func.name.clone(), "phpunit", TestKind::Hook, suite));
            }
        }
        Lang::Swift | Lang::ObjC => {
            let xctest = inherits_any(bases, suite.as_deref(), &["XCTestCase"])
                || suite.as_deref().is_some_and(is_test_type_name)
                || in_test_file;
            if xctest && starts_upper_after(&func.name, "test") {
                return Some((func.name.clone(), "xctest", TestKind::Case, suite));
            }
            if xctest
                && matches!(
                    func.name.as_str(),
                    "setUp" | "tearDown" | "setUpWithError" | "tearDownWithError"
                )
            {
                return Some((func.name.clone(), "xctest", TestKind::Hook, suite));
            }
        }
        Lang::Bash => {
            // shunit2 runs `testXxx`; bats headers arrive as functions whose
            // name is the description string (see lang/bash.rs).
            if file.path.ends_with(".bats") {
                if matches!(
                    func.name.as_str(),
                    "setup" | "teardown" | "setup_file" | "teardown_file"
                ) {
                    return Some((func.name.clone(), "bats", TestKind::Hook, None));
                }
                if !func.is_toplevel {
                    return Some((func.name.clone(), "bats", TestKind::Case, None));
                }
            }
            if starts_upper_after(&func.name, "test") {
                return Some((func.name.clone(), "shunit2", TestKind::Case, None));
            }
            if in_test_file
                && matches!(
                    func.name.as_str(),
                    "setUp" | "tearDown" | "oneTimeSetUp" | "oneTimeTearDown"
                )
            {
                return Some((func.name.clone(), "shunit2", TestKind::Hook, None));
            }
        }
        Lang::C => {
            // No dominant C framework; `test_*` in a test file is the shared
            // convention across CUnit, Unity, greatest and hand-rolled mains.
            if in_test_file && func.name.starts_with("test_") {
                return Some((func.name.clone(), "c-test", TestKind::Case, suite));
            }
        }
        _ => {}
    }
    None
}

/// Framework + kind implied by a function's annotations.
fn framework_for_decorations(decos: &[&str], language: Lang) -> Option<(&'static str, TestKind)> {
    // Cases first: an annotation like `@Test` wins over a co-located
    // `@DisplayName`, and `[Theory]` over its `[InlineData]` rows.
    for d in decos {
        let hit = match (*d, language) {
            ("test", Lang::Rust) | ("bench", Lang::Rust) => Some(("rust-test", TestKind::Case)),
            ("test", Lang::Swift) | ("Test", Lang::Swift) => {
                Some(("swift-testing", TestKind::Case))
            }
            ("Test", Lang::Java | Lang::Kotlin)
            | ("ParameterizedTest", _)
            | ("RepeatedTest", _)
            | ("TestFactory", _)
            | ("TestTemplate", _) => Some(("junit", TestKind::Case)),
            ("Fact", _) | ("Theory", _) => Some(("xunit", TestKind::Case)),
            ("TestMethod", _) => Some(("mstest", TestKind::Case)),
            ("Test", Lang::CSharp) | ("TestCase", Lang::CSharp) => Some(("nunit", TestKind::Case)),
            ("Test", Lang::Php) => Some(("phpunit", TestKind::Case)),
            ("test", _) => Some(("rust-test", TestKind::Case)),
            _ if d.starts_with("pytest.mark") || *d == "parametrize" => {
                Some(("pytest", TestKind::Case))
            }
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    // Then scaffolding.
    for d in decos {
        let hit = match (*d, language) {
            ("BeforeEach" | "AfterEach" | "BeforeAll" | "AfterAll" | "Before" | "After", _) => {
                Some(("junit", TestKind::Hook))
            }
            ("SetUp" | "TearDown" | "OneTimeSetUp" | "OneTimeTearDown", _) => {
                Some(("nunit", TestKind::Hook))
            }
            ("TestInitialize" | "TestCleanup", _) => Some(("mstest", TestKind::Hook)),
            ("fixture", Lang::Python) => Some(("pytest", TestKind::Hook)),
            ("TestFixture", Lang::CSharp) => Some(("nunit", TestKind::Suite)),
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    // `pytest.fixture` arrives dotted.
    if decos.contains(&"fixture") && language == Lang::Python {
        return Some(("pytest", TestKind::Hook));
    }
    None
}

// ---------------------------------------------------------------------------
// Block-style (BDD) detection
// ---------------------------------------------------------------------------

/// `describe("x", () => { it("y", ...) })` and friends: the case is a call
/// with a string-literal first argument, not a declaration. Suites nest by
/// byte containment, so an `it` reports the innermost `describe` around it.
fn collect_block_tests(
    idx: &mut TestIndex,
    func: &FunctionInfo,
    file: &FileInfo,
    calls: &[RawCall],
    fn_id: u32,
) {
    let bdd = matches!(
        func.language,
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx | Lang::Ruby | Lang::Cpp
    );
    if !bdd {
        return;
    }
    let framework = block_framework(func.language, file);

    // (start_byte, end_byte, suite name) for enclosing-group lookup.
    let mut groups: Vec<(u32, u32, String)> = Vec::new();
    for call in calls {
        if let Some((_, name)) = block_call(call, func.language)
            && is_group_call(call, func.language)
        {
            groups.push((call.start_byte, call.end_byte, name));
        }
    }

    for call in calls {
        let Some((kind, name)) = block_call(call, func.language) else {
            continue;
        };
        let suite = groups
            .iter()
            .filter(|(s, e, _)| *s < call.start_byte && *e >= call.end_byte)
            .min_by_key(|(s, e, _)| e - s)
            .map(|(_, _, n)| n.clone());
        push(
            idx,
            name,
            framework,
            kind,
            func.file_id,
            call.line,
            suite,
            Some(fn_id),
        );
    }
}

/// The case/suite name a BDD call declares, if it is one.
fn block_call(call: &RawCall, language: Lang) -> Option<(TestKind, String)> {
    let name = block_name(call, language)?;
    let kind = if is_group_call(call, language) {
        TestKind::Suite
    } else {
        TestKind::Case
    };
    Some((kind, name))
}

fn block_name(call: &RawCall, language: Lang) -> Option<String> {
    if !is_block_callee(call, language) {
        return None;
    }
    let lit = call
        .arg_lits
        .iter()
        .find(|l| l.index == 0 && matches!(l.kind, LitKind::Str))?;
    if lit.text.trim().is_empty() {
        return None;
    }
    Some(lit.text.clone())
}

/// Is this call one of the framework's block functions — including the
/// `it.only` / `test.skip` / `describe.each` modifier forms, where the callee
/// NAME is the modifier and the receiver is the block function?
fn is_block_callee(call: &RawCall, language: Lang) -> bool {
    let cases: &[&str] = match language {
        Lang::Cpp => &["TEST_CASE", "SCENARIO", "SECTION"],
        // Rails' `test "name" do` (ActiveSupport::TestCase) sits alongside the
        // RSpec vocabulary.
        Lang::Ruby => &[
            "it", "specify", "example", "scenario", "test", "describe", "context", "feature",
        ],
        _ => &["it", "test", "describe", "context", "suite", "bench"],
    };
    if cases.contains(&call.name.as_str()) {
        // A block function is called bare, never on a receiver: `str.test(re)`
        // and `regex.test(str)` are RegExp/String methods that would otherwise
        // read as Jest cases in every bundled vendor file. `t.Run("name", fn)`
        // in Go is likewise a subtest, not a top-level case.
        if call.receiver.is_some() {
            return false;
        }
        // `it("name", fn)` always passes a body; `re.test(s)`-shaped one-arg
        // calls that slipped through a missing receiver do not.
        return call.arg_count >= 2 || matches!(language, Lang::Cpp | Lang::Ruby);
    }
    const MODIFIERS: &[&str] = &[
        "only",
        "skip",
        "todo",
        "each",
        "concurrent",
        "failing",
        "sequential",
        "fails",
    ];
    match (&call.receiver, language) {
        (Some(recv), Lang::JavaScript | Lang::TypeScript | Lang::Tsx) => {
            MODIFIERS.contains(&call.name.as_str()) && cases.contains(&recv.as_str())
        }
        _ => false,
    }
}

fn is_group_call(call: &RawCall, language: Lang) -> bool {
    let groups: &[&str] = match language {
        Lang::Cpp => &["SCENARIO"],
        Lang::Ruby => &["describe", "context", "feature"],
        _ => &["describe", "context", "suite"],
    };
    if groups.contains(&call.name.as_str()) {
        return true;
    }
    call.receiver
        .as_deref()
        .is_some_and(|r| groups.contains(&r))
}

/// Which BDD runner a file uses, from its imports; the family default when
/// nothing named is imported (a Jest file imports nothing — its globals are
/// injected).
fn block_framework(language: Lang, file: &FileInfo) -> &'static str {
    match language {
        Lang::Ruby => "rspec",
        Lang::Cpp => "catch2",
        _ => {
            for import in &file.imports {
                let p = import.path.as_str();
                if p.contains("vitest") {
                    return "vitest";
                }
                if p.contains("jest") {
                    return "jest";
                }
                if p == "mocha" {
                    return "mocha";
                }
                if p == "node:test" || p == "test" {
                    return "node-test";
                }
            }
            "jest"
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `TEST(WaypointTest, DescribeIncludesAltitude) {` -> (suite, case).
fn gtest_names(signature: &str) -> Option<(String, String)> {
    let open = signature.find('(')?;
    let close = signature[open..].find(')')? + open;
    let mut parts = signature[open + 1..close].split(',');
    let suite = parts.next()?.trim().to_string();
    let case = parts.next()?.trim().to_string();
    if suite.is_empty() || case.is_empty() {
        return None;
    }
    Some((suite, case))
}

/// `TestFoo` matches prefix `Test`; `Testing` does not (next char must start a
/// new word), and neither does a bare `Test`.
fn starts_upper_after(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn last_segment(name: &str) -> &str {
    if name.starts_with("pytest.mark") {
        return name;
    }
    name.rsplit(['.', ':']).next().unwrap_or(name)
}

fn inherits_any(
    bases: Option<&FxHashMap<&str, Vec<&str>>>,
    ty: Option<&str>,
    wanted: &[&str],
) -> bool {
    let (Some(map), Some(ty)) = (bases, ty) else {
        return false;
    };
    map.get(ty).is_some_and(|bs| {
        bs.iter()
            .any(|b| wanted.iter().any(|w| b == w || b.ends_with(w)))
    })
}

/// `CartTests` / `CartTest` / `CartSpec` — a type whose name declares itself.
fn is_test_type_name(name: &str) -> bool {
    name.ends_with("Tests") || name.ends_with("Test") || name.ends_with("Spec")
}
