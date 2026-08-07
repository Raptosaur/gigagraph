use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-ruby. Notable grammar shapes (verified via
// `cargo run --example dump`):
// - `method` / `singleton_method` (`def self.foo`) both have `name`
//   (identifier) and an *optional* `parameters` field (`method_parameters`;
//   absent for paren-less `def foo`). No `@func.params` capture here — the
//   extractor's `child_by_field_name("parameters")` fallback handles both
//   forms, so a single pattern per def kind suffices.
// - `call` covers every invocation shape: `helper(x)`, paren-less `puts msg`
//   (still a `call` with an `argument_list`), receiver calls `obj.m(x)`
//   (`receiver` field), no-arg receiver calls `inv.total`, and block calls
//   `list.each do |x| .. end` / `loop do .. end` (`block` field, `arguments`
//   possibly absent). Truly bare `foo` with no args/receiver/block parses as
//   a plain `identifier`, not a `call` — inherently uncapturable.
// - Chained receivers (`@items.map { .. }.join`) capture the inner `call` as
//   `@call.recv`; the extractor discards receiver text containing `(`/newline.
// - `require "json"` / `require_relative "helper"` are ordinary `call` nodes;
//   they're matched by method-identifier text via `#eq?`. The method
//   identifier is captured as `@import.name` so the classifier can tell
//   `require` from `require_relative` (names hack — see spec docs).
const QUERY: &str = r#"
(method
  name: (identifier) @func.name) @func.def

(singleton_method
  name: (identifier) @func.name) @func.def

(call
  method: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(call
  receiver: (_) @call.recv
  method: (identifier) @call.name) @call

(call
  method: (identifier) @call.name
  block: (_)) @call

((call
   method: (identifier) @import.name
   arguments: (argument_list . (string (string_content) @import.path))) @import
  (#eq? @import.name "require"))

((call
   method: (identifier) @import.name
   arguments: (argument_list . (string (string_content) @import.path))) @import
  (#eq? @import.name "require_relative"))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "constant"];

const TYPE_KINDS: &[(&str, &str)] = &[("class", "name"), ("module", "name")];

const LOOP_KINDS: &[&str] = &["while", "until", "for", "while_modifier", "until_modifier"];

const BRANCH_KINDS: &[&str] = &[
    "if",
    "elsif",
    "unless",
    "case",
    "conditional",
    "if_modifier",
    "unless_modifier",
    "rescue",
];

// `self.helper` keeps receiver `self`; the resolver's this/self logic maps it
// back to the containing type, so no builtin receivers are needed.
const BUILTIN_RECEIVERS: &[&str] = &[];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Ruby,
        tree_sitter_ruby::LANGUAGE.into(),
        &["rb"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        BUILTIN_RECEIVERS,
    )
}

const STRING_KINDS: &[&str] = &["string", "simple_symbol"];
