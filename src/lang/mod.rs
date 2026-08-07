//! Language registry. Each language module provides a [`LangSpec`] describing
//! its tree-sitter grammar plus a query using the standard capture contract
//! (see docs/QUERY_CONTRACT.md):
//!
//! - `@func.def`   whole definition node (range = function span)
//! - `@func.name`  function name node
//! - `@func.params` parameter-list node (optional; param count = named children)
//! - `@func.body`  value node holding params when `func.params` can't be
//!                 captured directly (optional)
//! - `@call`       whole call node
//! - `@call.name`  callee simple-name node
//! - `@call.recv`  receiver node for method calls (optional)
//! - `@call.args`  argument-list node (optional; arg count = named children)
//! - `@import`         whole import node
//! - `@import.path`        module path / header (quotes stripped by extractor)
//! - `@import.path.system` same, but marks a system include (C `<...>`)
//! - `@import.name`    local names bound by the import (repeatable)
//! - `@package.name`   declared package/module name node

use crate::types::{ImportStyle, Lang};
use std::collections::HashMap;
use std::sync::OnceLock;
use tree_sitter::{Language, Query};

mod bash;
mod c;
mod cpp;
mod csharp;
mod go;
mod graphql;
mod java;
mod javascript;
mod kotlin;
mod objc;
mod openapi;
mod php;
mod prisma;
mod python;
mod ruby;
mod rust;
mod sql;
mod swift;
mod typescript;

pub struct LangSpec {
    pub lang: Lang,
    pub language: Language,
    pub extensions: &'static [&'static str],
    pub query: Query,
    /// Node kinds that count as identifiers for the semantic feature bag.
    pub identifier_kinds: &'static [&'static str],
    /// Node kinds holding string literals — harvested as call `ArgLit`s for
    /// endpoint/client detection.
    pub string_kinds: &'static [&'static str],
    /// (node kind, name field) pairs for enclosing-type detection.
    pub type_kinds: &'static [(&'static str, &'static str)],
    /// Node kinds counted as loops for control-flow features.
    pub loop_kinds: &'static [&'static str],
    /// Node kinds counted as branches for control-flow features.
    pub branch_kinds: &'static [&'static str],
    pub import_style: ImportStyle,
    /// Receivers that denote language/stdlib globals (`console`, `System`).
    /// Calls through them are attributed to `builtin:<receiver>`.
    pub builtin_receivers: &'static [&'static str],
}

impl LangSpec {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lang: Lang,
        language: Language,
        extensions: &'static [&'static str],
        query_src: &str,
        identifier_kinds: &'static [&'static str],
        string_kinds: &'static [&'static str],
        type_kinds: &'static [(&'static str, &'static str)],
        loop_kinds: &'static [&'static str],
        branch_kinds: &'static [&'static str],
        import_style: ImportStyle,
        builtin_receivers: &'static [&'static str],
    ) -> LangSpec {
        let query = Query::new(&language, query_src).unwrap_or_else(|e| {
            panic!("invalid query for {}: {e}", lang.name());
        });
        LangSpec {
            lang,
            language,
            extensions,
            query,
            identifier_kinds,
            string_kinds,
            type_kinds,
            loop_kinds,
            branch_kinds,
            import_style,
            builtin_receivers,
        }
    }
}

fn registry() -> &'static Vec<LangSpec> {
    static REG: OnceLock<Vec<LangSpec>> = OnceLock::new();
    REG.get_or_init(|| {
        vec![
            c::spec(),
            javascript::spec(),
            typescript::spec(),
            typescript::spec_tsx(),
            java::spec(),
            kotlin::spec(),
            swift::spec(),
            rust::spec(),
            bash::spec(),
            python::spec(),
            go::spec(),
            ruby::spec(),
            cpp::spec(),
            csharp::spec(),
            php::spec(),
            sql::spec(),
            prisma::spec(),
            graphql::spec(),
            openapi::spec(),
            objc::spec(),
        ]
    })
}

fn ext_map() -> &'static HashMap<&'static str, usize> {
    static MAP: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, spec) in registry().iter().enumerate() {
            for ext in spec.extensions {
                m.insert(*ext, i);
            }
        }
        m
    })
}

pub fn spec_for_ext(ext: &str) -> Option<&'static LangSpec> {
    ext_map().get(ext).map(|&i| &registry()[i])
}

pub fn spec_for_lang(lang: Lang) -> Option<&'static LangSpec> {
    registry().iter().find(|s| s.lang == lang)
}

pub fn supported_extensions() -> Vec<&'static str> {
    ext_map().keys().copied().collect()
}
