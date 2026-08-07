//! Code graph construction: id assignment, import classification, and
//! heuristic cross-file call resolution.

use crate::extract::{ExtractedFile, RawCall};
use crate::lang;
use crate::types::*;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GigaGraph {
    pub root: String,
    pub files: Vec<FileInfo>,
    pub functions: Vec<FunctionInfo>,
    pub calls: Vec<CallSite>,
    /// function id -> indices into `calls`
    pub calls_by_caller: Vec<Vec<u32>>,
    /// callee function id -> indices into `calls`
    pub callers_of: FxHashMap<u32, Vec<u32>>,
    /// simple name -> function ids
    pub name_index: FxHashMap<String, Vec<u32>>,
    /// qualified name -> function ids (overloads share a qname)
    pub qname_index: FxHashMap<String, Vec<u32>>,
    /// relative path -> file id
    pub path_index: FxHashMap<String, u32>,
    /// external package -> indices into `calls`
    pub package_calls: FxHashMap<String, Vec<u32>>,
    /// Detected API endpoints, outbound HTTP calls, and their correlation.
    pub endpoints: crate::endpoints::EndpointIndex,
    /// Every callee name that appears at any call site, resolved or not.
    /// A function whose name is here is reachable by some dynamic dispatch we
    /// could not resolve — never a dead-code candidate.
    pub called_names: FxHashSet<String>,
    /// Function names referenced as bare identifiers outside their own
    /// definitions (callbacks passed by value, re-exports, import bindings).
    pub referenced_names: FxHashSet<String>,
    /// React Native bridge: JS call sites matched to native implementations.
    pub bridge: crate::bridge::BridgeIndex,
}

/// Per-file input to the graph build: relative path, content hash, extraction.
pub struct FileInput {
    pub path: String,
    pub content_hash: u64,
    pub extracted: ExtractedFile,
}

impl GigaGraph {
    /// Builds the graph plus the per-function semantic feature bags (returned
    /// separately; they feed the vectorizer and are then dropped).
    pub fn build(
        root: String,
        mut inputs: Vec<FileInput>,
    ) -> (GigaGraph, Vec<FxHashMap<String, u32>>) {
        inputs.sort_by(|a, b| a.path.cmp(&b.path));

        let mut g = GigaGraph {
            root,
            ..Default::default()
        };
        let mut features: Vec<FxHashMap<String, u32>> = Vec::new();
        // Raw calls per function, kept aside until resolution.
        let mut raw_calls: Vec<Vec<RawCall>> = Vec::new();
        let mut decorations: Vec<Vec<crate::extract::RawDecoration>> = Vec::new();
        let mut ret_strs: Vec<Vec<String>> = Vec::new();
        // (file_id, start_byte, end_byte) per function for same-file ranking.
        let mut fn_files: Vec<u32> = Vec::new();

        let declared_packages: FxHashSet<String> = inputs
            .iter()
            .filter_map(|i| i.extracted.package.clone())
            .collect();

        for input in inputs {
            let file_id = g.files.len() as u32;
            let scope = input
                .extracted
                .package
                .clone()
                .unwrap_or_else(|| strip_extension(&input.path).to_string());
            let test_file = is_test_file(&input.path);
            for mut ef in input.extracted.functions {
                let fn_id = g.functions.len() as u32;
                let fn_decorations = std::mem::take(&mut ef.decorations);
                let is_test = test_file
                    || fn_decorations.iter().any(|d| is_test_decoration(&d.name))
                    || is_test_name(&ef.name, input.extracted.language);
                let qualified_name = match &ef.containing_type {
                    Some(t) => format!("{scope}::{t}::{}", ef.name),
                    None => format!("{scope}::{}", ef.name),
                };
                g.name_index.entry(ef.name.clone()).or_default().push(fn_id);
                g.qname_index
                    .entry(qualified_name.clone())
                    .or_default()
                    .push(fn_id);
                g.functions.push(FunctionInfo {
                    id: fn_id,
                    name: std::mem::take(&mut ef.name),
                    qualified_name,
                    file_id,
                    language: input.extracted.language,
                    start_line: ef.start_line,
                    end_line: ef.end_line,
                    signature: std::mem::take(&mut ef.signature),
                    containing_type: ef.containing_type.take(),
                    param_count: ef.param_count,
                    is_toplevel: ef.is_toplevel,
                    has_decorations: !fn_decorations.is_empty(),
                    is_exported: ef.is_exported,
                    is_test,
                });
                raw_calls.push(std::mem::take(&mut ef.calls));
                decorations.push(fn_decorations);
                ret_strs.push(std::mem::take(&mut ef.ret_strs));
                features.push(std::mem::take(&mut ef.features));
                fn_files.push(file_id);
            }
            g.path_index.insert(input.path.clone(), file_id);
            g.files.push(FileInfo {
                id: file_id,
                path: input.path,
                language: input.extracted.language,
                package: input.extracted.package,
                imports: input.extracted.imports,
                consts: input.extracted.consts,
                content_hash: input.content_hash,
            });
        }

        // ---- Import classification (external vs internal) ----
        let path_index = g.path_index.clone();
        let file_paths: Vec<String> = g.files.iter().map(|f| f.path.clone()).collect();
        for file in &mut g.files {
            let style = lang::spec_for_lang(file.language)
                .map(|s| s.import_style)
                .unwrap_or(ImportStyle::PathLike);
            let dir = parent_dir(&file.path).to_string();
            for imp in &mut file.imports {
                classify_import(
                    imp,
                    style,
                    file.language,
                    &dir,
                    &path_index,
                    &file_paths,
                    &declared_packages,
                );
            }
        }

        // ---- Call resolution (parallel per function) ----
        let resolved: Vec<Vec<CallSite>> = raw_calls
            .par_iter()
            .enumerate()
            .map(|(fn_id, calls)| {
                calls
                    .iter()
                    .map(|c| {
                        let resolution = resolve_call(&g, fn_id as u32, c);
                        CallSite {
                            caller: fn_id as u32,
                            name: c.name.clone(),
                            receiver: c.receiver.clone(),
                            line: c.line,
                            arg_count: c.arg_count,
                            resolution,
                        }
                    })
                    .collect()
            })
            .collect();

        g.calls_by_caller = vec![Vec::new(); g.functions.len()];
        for (fn_id, sites) in resolved.into_iter().enumerate() {
            for site in sites {
                let idx = g.calls.len() as u32;
                match &site.resolution {
                    Resolution::Internal { callee, .. } => {
                        g.callers_of.entry(*callee).or_default().push(idx);
                    }
                    Resolution::External { package } => {
                        g.package_calls
                            .entry(package.clone())
                            .or_default()
                            .push(idx);
                        *features[fn_id].entry(format!("pkg:{package}")).or_insert(0) += 1;
                    }
                    Resolution::Unresolved => {}
                }
                g.calls_by_caller[fn_id].push(idx);
                g.calls.push(site);
            }
        }

        // Resolve single-assignment string constants into call/decoration
        // arguments: `fetch(API)` gains a Str lit carrying API's value, so
        // every downstream detector sees the literal path. Done once here
        // rather than per-detector.
        for (fn_id, calls) in raw_calls.iter_mut().enumerate() {
            let consts = &g.files[g.functions[fn_id].file_id as usize].consts;
            if consts.is_empty() {
                continue;
            }
            for call in calls.iter_mut() {
                substitute_consts(&mut call.arg_lits, consts);
            }
        }
        for (fn_id, decos) in decorations.iter_mut().enumerate() {
            let consts = &g.files[g.functions[fn_id].file_id as usize].consts;
            if consts.is_empty() {
                continue;
            }
            for d in decos.iter_mut() {
                substitute_consts(&mut d.arg_lits, consts);
            }
        }

        // Endpoint + client-call detection needs classified imports and the
        // raw calls (with literal args) that CallSite no longer carries.
        g.endpoints = crate::endpoints::detect(
            &g.files,
            &g.functions,
            &raw_calls,
            &decorations,
            &g.name_index,
        );

        g.bridge =
            crate::bridge::detect(&g.files, &g.functions, &raw_calls, &decorations, &ret_strs);

        // ---- Reference inventory for dead-code analysis ----
        // Every callee name, resolved or not: dynamic dispatch we couldn't
        // resolve still names its target.
        for calls in &raw_calls {
            for c in calls {
                g.called_names.insert(c.name.clone());
            }
        }
        // Function names appearing as bare identifiers in OTHER functions'
        // bodies (callback passed by value, re-export) or bound by imports.
        // Same-named definitions don't shield each other: a function's own
        // occurrences are skipped by name.
        for (fn_id, bag) in features.iter().enumerate() {
            let own = &g.functions[fn_id].name;
            for key in bag.keys() {
                if let Some(ident) = key.strip_prefix("id:") {
                    if ident != own && g.name_index.contains_key(ident) {
                        g.referenced_names.insert(ident.to_string());
                    }
                }
            }
        }
        for file in &g.files {
            for imp in &file.imports {
                for n in &imp.names {
                    if g.name_index.contains_key(n) {
                        g.referenced_names.insert(n.clone());
                    }
                }
            }
        }

        (g, features)
    }

    pub fn file_of(&self, fn_id: u32) -> &FileInfo {
        &self.files[self.functions[fn_id as usize].file_id as usize]
    }

    /// External packages a function's body calls into.
    pub fn packages_used(&self, fn_id: u32) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(idxs) = self.calls_by_caller.get(fn_id as usize) {
            for &i in idxs {
                if let Resolution::External { package } = &self.calls[i as usize].resolution {
                    if !out.contains(package) {
                        out.push(package.clone());
                    }
                }
            }
        }
        out.sort();
        out
    }
}

/// After each identifier argument naming a known string constant, insert a
/// synthetic Str lit with the constant's value (same index, so positional
/// helpers see it where the identifier stood).
fn substitute_consts(lits: &mut Vec<crate::extract::ArgLit>, consts: &[(String, String)]) {
    use crate::extract::{ArgLit, LitKind};
    let mut insertions: Vec<(usize, ArgLit)> = Vec::new();
    for (i, lit) in lits.iter().enumerate() {
        if lit.kind == LitKind::Ident && lit.key.is_none() {
            if let Some((_, v)) = consts.iter().find(|(n, _)| *n == lit.text) {
                insertions.push((
                    i + 1,
                    ArgLit {
                        index: lit.index,
                        key: None,
                        kind: LitKind::Str,
                        text: v.clone(),
                    },
                ));
            }
        }
    }
    for (offset, (pos, lit)) in insertions.into_iter().enumerate() {
        lits.insert(pos + offset, lit);
    }
}

/// Is this path a test file by ecosystem convention? Directory components
/// (`tests/`, `__tests__/`, `spec/`) or file naming (`*.test.ts`,
/// `*_test.go`, `test_*.py`, `*Test.java`, `*_spec.rb`, `conftest.py`).
pub fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.split('/').rev().skip(1).any(|c| {
        matches!(
            c,
            "tests" | "test" | "__tests__" | "spec" | "specs" | "testing"
        )
    }) {
        return true;
    }
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = strip_extension(base);
    // `*Test.java` / `*Tests.cs` style class-per-file suffixes only count in
    // ecosystems that use them; `contest.rs` must not match.
    let suffix_langs = [".java", ".kt", ".cs", ".scala", ".swift"];
    let class_suffix = (stem.ends_with("test") && stem.len() > 4
        || stem.ends_with("tests") && stem.len() > 5)
        && suffix_langs.iter().any(|e| lower.ends_with(e));
    base.contains(".test.")
        || base.contains(".spec.")
        || base == "conftest.py"
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.starts_with("test_")
        || class_suffix
}

/// Decoration/annotation/attribute names that mark a function as a test.
/// Matched on the last dotted segment: `pytest.mark.parametrize`,
/// `tokio::test`, `org.junit.Test` all qualify.
fn is_test_decoration(name: &str) -> bool {
    if name.starts_with("pytest.mark") {
        return true;
    }
    let last = name.rsplit(['.', ':']).next().unwrap_or(name);
    matches!(
        last,
        "test"
            | "Test"
            | "TestCase"
            | "TestMethod"
            | "TestFixture"
            | "ParameterizedTest"
            | "RepeatedTest"
            | "Fact"
            | "Theory"
            | "fixture"
    )
}

/// Name conventions strong enough on their own: Python/Ruby `test_*`;
/// Go `TestXxx`/`BenchmarkXxx` (the compiler enforces the shape only in
/// `_test.go` files, which `is_test_file` already catches — the name check
/// covers helpers referenced across test packages).
fn is_test_name(name: &str, language: Lang) -> bool {
    match language {
        Lang::Python | Lang::Ruby | Lang::Php => name.starts_with("test_"),
        Lang::Go => {
            for prefix in ["Test", "Benchmark", "Fuzz"] {
                if let Some(rest) = name.strip_prefix(prefix) {
                    if rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

fn strip_extension(path: &str) -> &str {
    match path.rfind('.') {
        Some(i) if i > path.rfind('/').map_or(0, |s| s + 1) => &path[..i],
        _ => path,
    }
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_import(
    imp: &mut Import,
    style: ImportStyle,
    language: Lang,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
    file_paths: &[String],
    declared_packages: &FxHashSet<String>,
) {
    match style {
        ImportStyle::PathLike => match language {
            Lang::C | Lang::Cpp => {
                if imp.system {
                    imp.external_package = Some(imp.path.clone());
                } else {
                    imp.resolved_file =
                        resolve_include(&imp.path, file_dir, path_index, file_paths);
                }
            }
            Lang::Bash => classify_bash(imp, file_dir, path_index, file_paths),
            Lang::Go => classify_go(imp, file_paths),
            Lang::Ruby => classify_ruby(imp, file_dir, path_index),
            _ => {
                // JS/TS
                if imp.path.starts_with('.') || imp.path.starts_with('/') {
                    imp.resolved_file = resolve_js_relative(&imp.path, file_dir, path_index);
                } else {
                    imp.external_package = Some(js_package_name(&imp.path));
                }
            }
        },
        ImportStyle::DottedPackage => match language {
            Lang::Rust => classify_rust(imp, file_dir, path_index, file_paths),
            Lang::Python => classify_python(imp, file_dir, path_index),
            Lang::Php => classify_php(imp, file_dir, path_index, declared_packages),
            _ => {
                let internal =
                    package_prefixes(&imp.path, '.').any(|p| declared_packages.contains(p));
                if !internal {
                    imp.external_package = Some(dotted_external_package(&imp.path, '.'));
                }
            }
        },
        ImportStyle::Module => {
            imp.external_package = Some(imp.path.clone());
        }
    }
}

/// Rust `use` paths: `crate::`/`self::`/`super::` are project-internal
/// (resolved against src layout); anything else is an external crate named by
/// the first segment (`std::mem::swap` -> `std`).
fn classify_rust(
    imp: &mut Import,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
    file_paths: &[String],
) {
    let segs: Vec<&str> = imp.path.split("::").filter(|s| !s.is_empty()).collect();
    let Some(&first) = segs.first() else { return };
    match first {
        "crate" | "self" | "super" => {
            let base = match first {
                "crate" => None,
                "self" => Some(file_dir.to_string()),
                _ => Some(parent_dir(file_dir).to_string()),
            };
            // Trailing segments may be items, not modules: drop them until a
            // file matches (`crate::graph::GigaGraph` -> src/graph.rs).
            for keep in (1..segs.len()).rev() {
                let rel = segs[1..=keep].join("/");
                let stems: Vec<String> = match &base {
                    Some(dir) => vec![normalize_path(dir, &rel)],
                    None => vec![format!("src/{rel}"), rel.clone()],
                };
                for stem in stems {
                    for cand in [format!("{stem}.rs"), format!("{stem}/mod.rs")] {
                        if let Some(&id) = path_index.get(&cand) {
                            imp.resolved_file = Some(id);
                            return;
                        }
                    }
                }
                // Nested crate in a monorepo: unique `**/src/<rel>.rs` match.
                if first == "crate" {
                    for suffix in [format!("/src/{rel}.rs"), format!("/src/{rel}/mod.rs")] {
                        let mut found: Option<u32> = None;
                        for (i, p) in file_paths.iter().enumerate() {
                            if p.ends_with(&suffix) {
                                if found.is_some() {
                                    found = None;
                                    break;
                                }
                                found = Some(i as u32);
                            }
                        }
                        if let Some(id) = found {
                            imp.resolved_file = Some(id);
                            return;
                        }
                    }
                }
            }
        }
        _ => imp.external_package = Some(first.to_string()),
    }
}

/// Python imports: leading dots walk up from the file's directory; absolute
/// dotted paths are tried against the tree (`a.b` -> a/b.py, a/b/__init__.py)
/// before falling back to an external package named by the first segment.
fn classify_python(imp: &mut Import, file_dir: &str, path_index: &FxHashMap<String, u32>) {
    let path = imp.path.as_str();
    let dots = path.chars().take_while(|&c| c == '.').count();
    let rest = &path[dots..];
    let segs: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();

    let base = if dots > 0 {
        let mut dir = file_dir.to_string();
        for _ in 1..dots {
            dir = parent_dir(&dir).to_string();
        }
        Some(dir)
    } else {
        None
    };

    // `from a.b import c`: c may be a symbol or a module — try longest first.
    let max_keep = segs.len();
    for keep in (0..=max_keep).rev() {
        if dots == 0 && keep == 0 {
            break;
        }
        let rel = segs[..keep].join("/");
        let prefix = match &base {
            Some(dir) if dir.is_empty() => rel.clone(),
            Some(dir) => {
                if rel.is_empty() {
                    dir.clone()
                } else {
                    format!("{dir}/{rel}")
                }
            }
            None => rel.clone(),
        };
        if prefix.is_empty() {
            continue;
        }
        for cand in [format!("{prefix}.py"), format!("{prefix}/__init__.py")] {
            if let Some(&id) = path_index.get(&cand) {
                imp.resolved_file = Some(id);
                return;
            }
        }
    }
    if dots == 0 {
        if let Some(&first) = segs.first() {
            imp.external_package = Some(first.to_string());
        }
    }
}

/// Go import strings: no slash means stdlib (`fmt`); otherwise internal when
/// the path's tail matches a directory in the tree, else external under the
/// full import path. The implicit package name (last segment) is bound so
/// receiver-directed attribution (`util.Helper()`) works.
fn classify_go(imp: &mut Import, file_paths: &[String]) {
    let path = imp.path.clone();
    if imp.names.is_empty() {
        if let Some(last) = path.rsplit('/').next() {
            imp.names.push(last.to_string());
        }
    }
    if !path.contains('/') {
        imp.external_package = Some(path);
        return;
    }
    let internal = file_paths.iter().any(|p| {
        let dir = parent_dir(p);
        !dir.is_empty() && (path == dir || path.ends_with(&format!("/{dir}")))
    });
    if !internal {
        imp.external_package = Some(path);
    }
}

/// Ruby: `require_relative` (marked via the captured method name) and dotted
/// paths resolve against the tree with an `.rb` suffix; `require` is external,
/// packaged by the first path segment.
fn classify_ruby(imp: &mut Import, file_dir: &str, path_index: &FxHashMap<String, u32>) {
    let relative = imp.names.iter().any(|n| n == "require_relative") || imp.path.starts_with('.');
    if relative {
        let base = normalize_path(file_dir, &imp.path);
        for cand in [base.clone(), format!("{base}.rb")] {
            if let Some(&id) = path_index.get(&cand) {
                imp.resolved_file = Some(id);
                return;
            }
        }
    } else {
        imp.external_package = Some(js_package_name(&imp.path));
    }
}

/// Bash `source`: paths with variables stay unresolved; otherwise resolve
/// relative to the sourcing script (falling back to a unique suffix match),
/// else treat as a command sourced from PATH.
fn classify_bash(
    imp: &mut Import,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
    file_paths: &[String],
) {
    if imp.path.contains('$') {
        return;
    }
    if let Some(id) = resolve_include(&imp.path, file_dir, path_index, file_paths) {
        imp.resolved_file = Some(id);
        return;
    }
    if !imp.path.contains('/') && !imp.path.ends_with(".sh") {
        imp.external_package = Some(imp.path.clone());
    }
}

/// PHP: `require`-style paths resolve like files; `use` declarations classify
/// by backslash-separated namespace against declared namespaces.
fn classify_php(
    imp: &mut Import,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
    declared_packages: &FxHashSet<String>,
) {
    let path = imp.path.clone();
    if path.contains('/') || path.ends_with(".php") {
        let base = normalize_path(file_dir, &path);
        if let Some(&id) = path_index.get(&base) {
            imp.resolved_file = Some(id);
        }
        return;
    }
    let internal = package_prefixes(&path, '\\').any(|p| declared_packages.contains(p));
    if !internal {
        imp.external_package = Some(dotted_external_package(&path, '\\'));
    }
}

/// `lodash/fp` -> `lodash`; `@scope/pkg/sub` -> `@scope/pkg`.
fn js_package_name(path: &str) -> String {
    let mut parts = path.split('/');
    let first = parts.next().unwrap_or(path);
    if first.starts_with('@') {
        match parts.next() {
            Some(second) => format!("{first}/{second}"),
            None => first.to_string(),
        }
    } else {
        first.to_string()
    }
}

/// Cumulative separated prefixes: `a.b.c` -> `a`, `a.b`, `a.b.c`.
fn package_prefixes(path: &str, sep: char) -> impl Iterator<Item = &str> {
    path.char_indices()
        .filter_map(move |(i, ch)| (ch == sep).then_some(&path[..i]))
        .chain(std::iter::once(path))
}

/// Strip trailing `*` and class-like (uppercase-initial) segments:
/// `java.util.List` -> `java.util`.
fn dotted_external_package(path: &str, sep: char) -> String {
    let segs: Vec<&str> = path.split(sep).collect();
    let mut keep = segs.len();
    while keep > 1 {
        let s = segs[keep - 1];
        let class_like = s == "*" || s.chars().next().map_or(false, |c| c.is_uppercase());
        if class_like {
            keep -= 1;
        } else {
            break;
        }
    }
    segs[..keep].join(".")
}

fn normalize_path(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = if rel.starts_with('/') {
        Vec::new()
    } else if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect()
    };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

const JS_RESOLVE_SUFFIXES: &[&str] = &[
    "",
    ".ts",
    ".tsx",
    ".mts",
    ".cts",
    ".js",
    ".jsx",
    ".mjs",
    ".cjs",
    "/index.ts",
    "/index.tsx",
    "/index.js",
    "/index.jsx",
];

fn resolve_js_relative(
    rel: &str,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
) -> Option<u32> {
    let base = normalize_path(file_dir, rel);
    for suffix in JS_RESOLVE_SUFFIXES {
        if let Some(&id) = path_index.get(&format!("{base}{suffix}")) {
            return Some(id);
        }
    }
    None
}

fn resolve_include(
    inc: &str,
    file_dir: &str,
    path_index: &FxHashMap<String, u32>,
    file_paths: &[String],
) -> Option<u32> {
    let joined = normalize_path(file_dir, inc);
    if let Some(&id) = path_index.get(&joined) {
        return Some(id);
    }
    if let Some(&id) = path_index.get(inc) {
        return Some(id);
    }
    // Fall back to unique suffix match anywhere in the tree.
    let suffix = format!("/{inc}");
    let mut found: Option<u32> = None;
    for (i, p) in file_paths.iter().enumerate() {
        if p.ends_with(&suffix) {
            if found.is_some() {
                return None; // ambiguous
            }
            found = Some(i as u32);
        }
    }
    found
}

/// Heuristic call resolution. Ranking:
/// same file > import-directed > same package > same directory >
/// same language > any language. External attribution via imports.
fn resolve_call(g: &GigaGraph, caller_id: u32, call: &RawCall) -> Resolution {
    let caller = &g.functions[caller_id as usize];
    let file = &g.files[caller.file_id as usize];
    let spec = lang::spec_for_lang(file.language);

    // --- Receiver-directed resolution ---
    if let Some(recv) = &call.receiver {
        let recv_base = recv.split('.').next().unwrap_or(recv);

        if matches!(recv_base, "this" | "self" | "$this" | "Self") {
            if let Some(t) = &caller.containing_type {
                if let Some(cands) = g.name_index.get(&call.name) {
                    let same_type: Vec<u32> = cands
                        .iter()
                        .copied()
                        .filter(|&id| {
                            g.functions[id as usize].containing_type.as_deref() == Some(t.as_str())
                        })
                        .collect();
                    if let Some(res) = pick(&same_type, |&id| {
                        g.functions[id as usize].file_id == caller.file_id
                    }) {
                        return res;
                    }
                    if !same_type.is_empty() {
                        return internal(&same_type, Confidence::Heuristic);
                    }
                }
            }
        }

        if let Some(spec) = spec {
            if spec.builtin_receivers.contains(&recv_base) {
                return Resolution::External {
                    package: format!("builtin:{recv_base}"),
                };
            }
        }

        // Receiver bound by an import? (`import * as fs from "fs"; fs.read()`
        // or `import com.foo.Bar; Bar.baz()`)
        for imp in &file.imports {
            let name_match = imp.names.iter().any(|n| n == recv_base)
                || (file.language != Lang::JavaScript
                    && file.language != Lang::TypeScript
                    && file.language != Lang::Tsx
                    && imp.path.split('.').next_back() == Some(recv_base));
            if !name_match {
                continue;
            }
            if let Some(pkg) = &imp.external_package {
                return Resolution::External {
                    package: pkg.clone(),
                };
            }
            if let Some(fid) = imp.resolved_file {
                if let Some(cands) = g.name_index.get(&call.name) {
                    let in_file: Vec<u32> = cands
                        .iter()
                        .copied()
                        .filter(|&id| g.functions[id as usize].file_id == fid)
                        .collect();
                    if !in_file.is_empty() {
                        return internal(&in_file, Confidence::High);
                    }
                }
            }
            // Java/Kotlin: `import com.foo.Bar` + `Bar.baz()` -> functions
            // whose containing type is Bar.
            if let Some(cands) = g.name_index.get(&call.name) {
                let typed: Vec<u32> = cands
                    .iter()
                    .copied()
                    .filter(|&id| {
                        g.functions[id as usize].containing_type.as_deref() == Some(recv_base)
                    })
                    .collect();
                if !typed.is_empty() {
                    return internal(&typed, Confidence::High);
                }
            }
        }

        // Static-style call through a known indexed type name: `Bar.baz()`.
        if recv_base.chars().next().is_some_and(|c| c.is_uppercase()) {
            if let Some(cands) = g.name_index.get(&call.name) {
                let typed: Vec<u32> = cands
                    .iter()
                    .copied()
                    .filter(|&id| {
                        g.functions[id as usize].containing_type.as_deref() == Some(recv_base)
                    })
                    .collect();
                if !typed.is_empty() {
                    return internal(&typed, Confidence::High);
                }
            }
        }
    }

    // --- Name-based resolution ---
    let empty: Vec<u32> = Vec::new();
    let cands: Vec<u32> = g
        .name_index
        .get(&call.name)
        .unwrap_or(&empty)
        .iter()
        .copied()
        .filter(|&id| !g.functions[id as usize].is_toplevel)
        .collect();

    if !cands.is_empty() {
        // 1. Same file.
        if let Some(res) = pick(&cands, |&id| {
            g.functions[id as usize].file_id == caller.file_id
        }) {
            return res;
        }
        // 2. Named-import match: `import {foo} from "./x"` directs `foo()`.
        for imp in &file.imports {
            if !imp.names.iter().any(|n| n == &call.name) {
                // Java static import: `import static com.Foo.bar` -> bar().
                let static_match = imp
                    .path
                    .rsplit_once('.')
                    .is_some_and(|(_, last)| last == call.name);
                if !static_match {
                    continue;
                }
            }
            if let Some(pkg) = &imp.external_package {
                return Resolution::External {
                    package: pkg.clone(),
                };
            }
            if let Some(fid) = imp.resolved_file {
                if let Some(res) = pick(&cands, |&id| g.functions[id as usize].file_id == fid) {
                    return res;
                }
            }
        }
        // 3. Same declared package.
        if let Some(pkg) = &file.package {
            if let Some(res) = pick(&cands, |&id| {
                g.file_of(id).package.as_deref() == Some(pkg.as_str())
            }) {
                return res;
            }
        }
        // 4. Same directory.
        let dir = parent_dir(&file.path);
        let same_dir: Vec<u32> = cands
            .iter()
            .copied()
            .filter(|&id| parent_dir(&g.file_of(id).path) == dir)
            .collect();
        if !same_dir.is_empty() {
            return internal(&same_dir, Confidence::Heuristic);
        }
        // 5. Same language, then any language.
        let same_lang: Vec<u32> = cands
            .iter()
            .copied()
            .filter(|&id| g.functions[id as usize].language == file.language)
            .collect();
        let pool = if same_lang.is_empty() {
            &cands
        } else {
            &same_lang
        };
        let conf = if pool.len() == 1 {
            Confidence::High
        } else {
            Confidence::Heuristic
        };
        return internal(pool, conf);
    }

    // --- External attribution by import names ---
    for imp in &file.imports {
        if imp.names.iter().any(|n| n == &call.name) {
            if let Some(pkg) = &imp.external_package {
                return Resolution::External {
                    package: pkg.clone(),
                };
            }
        }
    }

    Resolution::Unresolved
}

/// Filter candidates; if any survive, resolve High with ambiguity noted.
fn pick(cands: &[u32], pred: impl Fn(&u32) -> bool) -> Option<Resolution> {
    let hits: Vec<u32> = cands.iter().copied().filter(|id| pred(id)).collect();
    if hits.is_empty() {
        None
    } else {
        Some(internal(&hits, Confidence::High))
    }
}

fn internal(hits: &[u32], confidence: Confidence) -> Resolution {
    Resolution::Internal {
        callee: hits[0],
        confidence,
        ambiguous_with: hits[1..].iter().copied().take(8).collect(),
    }
}
