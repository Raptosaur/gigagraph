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

pub fn import_paths(file: &ExtractedFile) -> Vec<&str> {
    file.imports.iter().map(|i| i.path.as_str()).collect()
}
