//! Shared helpers for language extraction tests.

use gigagraph::extract::{ExtractedFile, ExtractedFunction, extract};
use gigagraph::lang;

pub fn extract_str(ext: &str, source: &str) -> ExtractedFile {
    let spec = lang::spec_for_ext(ext).unwrap_or_else(|| panic!("no spec for extension {ext}"));
    extract(spec, source).expect("extraction failed")
}

pub fn extract_fixture(ext: &str, rel: &str) -> ExtractedFile {
    let path = format!("{}/tests/fixtures/{rel}", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    extract_str(ext, &source)
}

pub fn func<'a>(file: &'a ExtractedFile, name: &str) -> &'a ExtractedFunction {
    file.functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();
            panic!("function `{name}` not extracted; got: {names:?}")
        })
}

pub fn has_call(f: &ExtractedFunction, callee: &str) -> bool {
    f.calls.iter().any(|c| c.name == callee)
}

pub fn assert_calls(f: &ExtractedFunction, callees: &[&str]) {
    for callee in callees {
        assert!(
            has_call(f, callee),
            "function `{}` should call `{callee}`; calls: {:?}",
            f.name,
            f.calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()
        );
    }
}

/// Every extracted function name, in source order, minus the synthetic
/// `(toplevel)` holder.
#[allow(dead_code)]
pub fn function_names(file: &ExtractedFile) -> Vec<&str> {
    file.functions
        .iter()
        .filter(|f| !f.is_toplevel)
        .map(|f| f.name.as_str())
        .collect()
}

/// Assert the extractor found EXACTLY this set of functions — no misses, no
/// phantoms. Order-insensitive but multiplicity-sensitive: two methods named
/// `latest` on different types must be listed twice.
///
/// Spot-check assertions (`assert!(names.contains(..))`) cannot catch a
/// regression that stops extracting something; set equality can. Use it on
/// fixtures whose contents you control.
#[allow(dead_code)]
pub fn assert_exact_functions(file: &ExtractedFile, expected: &[&str]) {
    let mut got: Vec<&str> = function_names(file);
    let mut want: Vec<&str> = expected.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    if got == want {
        return;
    }
    let missing: Vec<&&str> = want.iter().filter(|n| !got.contains(n)).collect();
    let extra: Vec<&&str> = got.iter().filter(|n| !want.contains(n)).collect();
    // Same names, different counts: report the whole list rather than an
    // empty diff.
    panic!(
        "function set mismatch\n  missing (defined but not extracted): {missing:?}\n  \
         unexpected (extracted but not defined): {extra:?}\n  got:  {got:?}\n  want: {want:?}"
    );
}

pub fn import_paths(file: &ExtractedFile) -> Vec<&str> {
    file.imports.iter().map(|i| i.path.as_str()).collect()
}

/// Declared type of a field/property, if captured: `field_of(&file,
/// "UserService", "repo") == Some("UserRepository")`.
#[allow(dead_code)]
pub fn field_of<'a>(file: &'a ExtractedFile, owner: &str, name: &str) -> Option<&'a str> {
    file.fields
        .iter()
        .find(|f| f.owner == owner && f.name == name)
        .map(|f| f.type_name.as_str())
}

/// Declared/constructed type of a typed local or parameter, if captured.
#[allow(dead_code)]
pub fn local_of<'a>(f: &'a ExtractedFunction, name: &str) -> Option<&'a str> {
    f.locals
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t.as_str())
}
