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
// - DI type captures: Ruby has no type annotations, so receiver-type
//   resolution relies on `Klass.new` construction shapes only —
//   `x = DbStore.new` becomes a typed local and `@store = DbStore.new` a
//   field of the enclosing class. The `instance_variable` node's text keeps
//   the `@` sigil, so the field-table key is `@store`; receiver text for a
//   later `@store.persist` is also `@store`, and the resolver's bare
//   receiver_binding branch passes it through (it rejects only
//   `.`/`-`/`:`/`[`/`$`, not `@`), so key and lookup stay consistent.
//   `@store = store` (plain param assignment in initialize) is NOT captured —
//   there is no declared param type to join against. `class Service < Base`
//   yields one hierarchy edge via the `superclass` field. A SCOPED
//   superclass (`class API < Grape::API`) is a `scope_resolution`, not a
//   `constant`, so it needs its own pattern — capturing the SCOPE constant
//   ("Grape"), because the extractor's clean_type would reduce the full
//   node to its last segment ("API"), which carries no framework signal;
//   endpoints.rs gates Grape detection on the resulting "implements:grape"
//   evidence. `class Twitter::API < ...` names are scope_resolutions too,
//   hence the alternation on @hier.type.
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

((assignment
   left: (identifier) @local.name
   right: (call
     receiver: (constant) @local.type
     method: (identifier) @_new))
  (#eq? @_new "new"))

((assignment
   left: (instance_variable) @field.name
   right: (call
     receiver: (constant) @field.type
     method: (identifier) @_new))
  (#eq? @_new "new"))

(class
  name: (constant) @hier.type
  superclass: (superclass (constant) @hier.base))

(class
  name: [(constant) (scope_resolution)] @hier.type
  superclass: (superclass (scope_resolution scope: (constant) @hier.base)))
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
