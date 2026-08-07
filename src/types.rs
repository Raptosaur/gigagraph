//! Core data model for the code graph.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    C,
    JavaScript,
    TypeScript,
    Tsx,
    Java,
    Kotlin,
    Swift,
    Rust,
    Bash,
    Python,
    Go,
    Ruby,
    Cpp,
    CSharp,
    Php,
    Sql,
    Prisma,
    Graphql,
    Yaml,
    ObjC,
}

impl Lang {
    pub fn name(&self) -> &'static str {
        match self {
            Lang::C => "c",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Java => "java",
            Lang::Kotlin => "kotlin",
            Lang::Swift => "swift",
            Lang::Rust => "rust",
            Lang::Bash => "bash",
            Lang::Python => "python",
            Lang::Go => "go",
            Lang::Ruby => "ruby",
            Lang::Cpp => "cpp",
            Lang::CSharp => "csharp",
            Lang::Php => "php",
            Lang::Sql => "sql",
            Lang::Prisma => "prisma",
            Lang::Graphql => "graphql",
            Lang::Yaml => "yaml",
            Lang::ObjC => "objc",
        }
    }

    pub fn from_name(s: &str) -> Option<Lang> {
        match s.to_ascii_lowercase().as_str() {
            "c" => Some(Lang::C),
            "javascript" | "js" => Some(Lang::JavaScript),
            "typescript" | "ts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "java" => Some(Lang::Java),
            "kotlin" | "kt" => Some(Lang::Kotlin),
            "swift" => Some(Lang::Swift),
            "rust" | "rs" => Some(Lang::Rust),
            "bash" | "sh" | "shell" => Some(Lang::Bash),
            "python" | "py" => Some(Lang::Python),
            "go" | "golang" => Some(Lang::Go),
            "ruby" | "rb" => Some(Lang::Ruby),
            "cpp" | "c++" | "cxx" => Some(Lang::Cpp),
            "csharp" | "cs" | "c#" => Some(Lang::CSharp),
            "php" => Some(Lang::Php),
            "sql" => Some(Lang::Sql),
            "prisma" => Some(Lang::Prisma),
            "graphql" | "gql" => Some(Lang::Graphql),
            "yaml" | "yml" | "openapi" => Some(Lang::Yaml),
            "objc" | "objectivec" | "objective-c" => Some(Lang::ObjC),
            _ => None,
        }
    }
}

/// How an import is written in the source language; drives external-vs-internal
/// classification and package attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportStyle {
    /// JS/TS: `import x from "./a"` vs `import x from "pkg"`; C `#include`.
    PathLike,
    /// Java/Kotlin: `import com.foo.Bar`.
    DottedPackage,
    /// Swift: `import Foundation`.
    Module,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Module path / header / package as written (quotes stripped).
    pub path: String,
    /// Local names bound by this import (aliases, imported symbols).
    pub names: Vec<String>,
    pub line: u32,
    /// `#include <...>` in C, marks system header.
    pub system: bool,
    /// Resolved after indexing: Some(pkg) when this import points outside the
    /// indexed project.
    pub external_package: Option<String>,
    /// Resolved after indexing: file id this import points at, when internal
    /// and resolvable to a single indexed file.
    pub resolved_file: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: u32,
    /// Path relative to index root, `/`-separated.
    pub path: String,
    pub language: Lang,
    /// Declared package/module (Java/Kotlin package decl), if any.
    pub package: Option<String>,
    pub imports: Vec<Import>,
    /// Single-assignment string constants (name -> literal) — fuels path
    /// resolution for `fetch(API)`-style call sites.
    #[serde(default)]
    pub consts: Vec<(String, String)>,
    /// Content hash for incremental reindex.
    pub content_hash: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub id: u32,
    pub name: String,
    /// `<scope>::<Type>::<name>` — scope is package decl or relative path.
    pub qualified_name: String,
    pub file_id: u32,
    pub language: Lang,
    pub start_line: u32,
    pub end_line: u32,
    /// First source line of the definition, trimmed.
    pub signature: String,
    /// Enclosing class/struct/interface/object name, if any.
    pub containing_type: Option<String>,
    pub param_count: u16,
    /// Synthetic function holding a file's top-level call sites.
    pub is_toplevel: bool,
    /// Carried a decorator/annotation/attribute — framework-managed, never a
    /// dead-code candidate.
    #[serde(default)]
    pub has_decorations: bool,
    /// Structurally exported (JS/TS export wrapper). Visibility keywords that
    /// appear in the signature line are sniffed separately.
    #[serde(default)]
    pub is_exported: bool,
    /// Test or test scaffolding: carries a test decoration (`#[test]`,
    /// `@Test`, `[Fact]`, `pytest.mark.*`), follows a test naming convention
    /// (`test_*`, `Test*` in Go `_test.go`), or is defined in a test file
    /// (`tests/`, `__tests__/`, `*.spec.ts`, `*_test.go`, ...). Helpers in
    /// test files count: changing what they call still dirties the file's
    /// test run.
    #[serde(default)]
    pub is_test: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    /// Unique candidate, or matched in same file/package.
    High,
    /// Name-based cross-project match (possibly ambiguous).
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Resolution {
    /// Call resolved to an indexed function.
    Internal {
        callee: u32,
        confidence: Confidence,
        /// Other plausible candidates (function ids) when ambiguous.
        ambiguous_with: Vec<u32>,
    },
    /// Call attributed to an external package (via imports).
    External {
        package: String,
    },
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub caller: u32,
    /// Callee simple name as written.
    pub name: String,
    /// Receiver text for method calls (`obj` in `obj.m()`), if simple.
    pub receiver: Option<String>,
    pub line: u32,
    pub arg_count: u16,
    pub resolution: Resolution,
}
