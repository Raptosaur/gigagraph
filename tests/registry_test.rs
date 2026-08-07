//! Forces every language spec (and its query) to compile. A bad node kind in
//! any query panics here rather than at index time.

#[test]
fn all_language_specs_compile() {
    for ext in [
        "c", "h", "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "java", "kt", "kts",
        "swift",
    ] {
        let spec = gigagraph::lang::spec_for_ext(ext)
            .unwrap_or_else(|| panic!("no spec registered for extension {ext}"));
        assert!(
            spec.query.pattern_count() > 0,
            "{} query has no patterns",
            spec.lang.name()
        );
    }
}
