use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Generic YAML (`.yaml` / `.yml`) via `tree-sitter-yaml`, indexed *shallowly*
// with OpenAPI specs as the motivating case. Verified against
// tests/fixtures/probe/probe.yaml (see tests/openapi_test.rs).
//
// Deliberate design: tree-sitter-yaml knows nothing about OpenAPI, and a pure
// query cannot express "the `paths:` section" without a pile of fragile
// key-name heuristics. So the mapping is structural only:
//
// - Each *top-level* mapping key (`openapi:`, `info:`, `paths:`,
//   `components:`, or `jobs:` in a CI file) becomes a `@func.def` named by
//   the key and spanning its section. That single pattern is depth-limited by
//   construction (`document > block_node > block_mapping > pair`), so nested
//   keys do NOT become functions.
// - No `@call`s at all: YAML keys like `get`/`post` must not create
//   name-resolved edges into real code. Deep OpenAPI endpoint extraction
//   belongs to the endpoint-mapping layer, not this query.
// - `string_scalar` identifiers inside a section feed the semantic feature
//   bag, so `/pets`, `listPets`, `Pet` are searchable via similarity.
//
// Tradeoff, noted: registering `.yaml`/`.yml` indexes *all* YAML a walk finds
// (CI configs, k8s manifests). The walk respects gitignore and this shallow
// mapping costs a handful of functions per file, so it is kept cheap rather
// than special-cased.
const QUERY: &str = r#"
(document
  (block_node
    (block_mapping
      (block_mapping_pair key: (flow_node) @func.name) @func.def)))
"#;

const IDENTIFIER_KINDS: &[&str] = &["string_scalar"];

const STRING_KINDS: &[&str] = &["double_quote_scalar", "single_quote_scalar"];

const TYPE_KINDS: &[(&str, &str)] = &[];

const LOOP_KINDS: &[&str] = &[];

const BRANCH_KINDS: &[&str] = &[];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Yaml,
        tree_sitter_yaml::LANGUAGE.into(),
        &["yaml", "yml"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        &[],
    )
}
