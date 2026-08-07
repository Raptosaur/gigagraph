use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Written against tree-sitter-python 0.25. Notable grammar shapes (verified
// via `cargo run --example dump`):
// - `function_definition` covers plain defs, `async def`, methods, and nested
//   defs; it always has `name:`, `parameters:`, and `body:` fields (`async` is
//   an anonymous token). Decorators wrap it in a `decorated_definition`, but
//   patterns match anywhere in the tree, so the inner `function_definition`
//   still matches and `@func.def` spans from `def` — signatures stay
//   `def ...`/`async def ...`. Lambdas are anonymous (no name field) and are
//   deliberately skipped as noise.
// - `call` always has `function:` and `arguments:` fields. The callee is an
//   `identifier` for plain and constructor calls (`Klass()`), or an
//   `attribute` (`object:` + `attribute:` fields) for receiver calls. Dotted
//   receivers (`os.path.join`) capture the whole object node text; chained /
//   `super()` receivers contain `(` and are discarded by the extractor's
//   receiver sanitizer, which is the intended fallback (the call itself is
//   still kept, e.g. `super().m()` yields callee `m` with no receiver).
// - `import_statement` binds a `dotted_name` or an `aliased_import`
//   (`name:` dotted_name + `alias:` identifier). `import_from_statement` has
//   a `module_name:` — a `dotted_name`, or a `relative_import` whose node
//   text keeps the leading dots (`.`, `..pkg`) — plus one `name:` child per
//   imported symbol (`dotted_name`, or `aliased_import` for `x as z`).
// - Python has no package declaration, so there is no `@package.name`.
// - Type captures (verified via probe dump): `typed_parameter` puts the bare
//   `identifier` first and wraps the annotation in a `type:` field whose
//   `(type ...)` node contains the actual `identifier` (generics become
//   `generic_type` inside `(type ...)` and simply don't match the
//   `(type (identifier))` shape — intended, `clean_type` rejects them anyway).
//   `typed_default_parameter` is the same but with a `name:` field. Annotated
//   assignments (`x: T = ...`) put the same `type: (type ...)` field on the
//   `assignment` node. Dotted annotations (`x: mod.T`) are an `attribute`
//   node inside `(type ...)` (probe-verified); each pattern has an
//   attribute twin capturing the WHOLE attribute node — its text is
//   dot-joined with no whitespace, so clean_type keeps the last segment
//   (`mod.deep.T` -> `T`). Constructed locals/fields (`x = Klass()`,
//   `self.x = Klass()`) capture the callee as the type, guarded by
//   `#match? "^[A-Z]"` because Python calls are case-blind — without the
//   guard every `x = helper()` would record `helper` as a type.
//   `self.x = param` captures the ctor PARAM NAME in `@field.type` position
//   (deferred join, same as PHP `$this->repo = $repo`): the graph build
//   substitutes the `__init__` parameter's declared type, so this pattern
//   stays UNGUARDED by case — the param name is lowercase by convention.
//   Hierarchy: `class_definition` lists bases in a `superclasses:`
//   `argument_list`; one `@hier.type`/`@hier.base` pair per base. Dotted
//   bases (`class X(abc.ABC)`) are `attribute` nodes captured whole, with
//   clean_type again keeping the last segment (`ABC`).
const QUERY: &str = r#"
(function_definition
  name: (identifier) @func.name
  parameters: (parameters) @func.params) @func.def

(call
  function: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(call
  function: (attribute
    object: (_) @call.recv
    attribute: (identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(decorator
  (call
    function: (attribute) @deco.name
    arguments: (argument_list) @deco.args)) @deco

(decorator
  (call
    function: (identifier) @deco.name
    arguments: (argument_list) @deco.args)) @deco

(decorator (attribute) @deco.name) @deco

(decorator (identifier) @deco.name) @deco

(import_statement
  name: (dotted_name) @import.path) @import

(import_statement
  name: (aliased_import
    name: (dotted_name) @import.path
    alias: (identifier) @import.name)) @import

(import_from_statement
  module_name: (dotted_name) @import.path) @import

(import_from_statement
  module_name: (relative_import) @import.path) @import

(import_from_statement
  name: (dotted_name) @import.name) @import

(import_from_statement
  name: (aliased_import
    alias: (identifier) @import.name)) @import

(typed_parameter
  (identifier) @local.name
  type: (type (identifier) @local.type))

(typed_parameter
  (identifier) @local.name
  type: (type (attribute) @local.type))

(typed_default_parameter
  name: (identifier) @local.name
  type: (type (identifier) @local.type))

(typed_default_parameter
  name: (identifier) @local.name
  type: (type (attribute) @local.type))

(assignment
  left: (identifier) @local.name
  type: (type (identifier) @local.type))

(assignment
  left: (identifier) @local.name
  type: (type (attribute) @local.type))

(assignment
  left: (identifier) @local.name
  right: (call
    function: (identifier) @local.type)
  (#match? @local.type "^[A-Z]"))

(assignment
  left: (attribute
    object: (identifier) @_self
    attribute: (identifier) @field.name)
  right: (call
    function: (identifier) @field.type)
  (#eq? @_self "self")
  (#match? @field.type "^[A-Z]"))

(assignment
  left: (attribute
    object: (identifier) @_self
    attribute: (identifier) @field.name)
  right: (identifier) @field.type
  (#eq? @_self "self"))

(class_definition
  name: (identifier) @hier.type
  superclasses: (argument_list (identifier) @hier.base))

(class_definition
  name: (identifier) @hier.type
  superclasses: (argument_list (attribute) @hier.base))

(module
  (expression_statement
    (assignment
      left: (identifier) @const.name
      right: (string) @const.value)))

(class_definition
  body: (block
    (expression_statement
      (assignment
        left: (identifier) @const.name
        right: (string) @const.value))))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[("class_definition", "name")];

// `for_in_clause` counts comprehension iteration as a loop.
const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "for_in_clause"];

const BRANCH_KINDS: &[&str] = &["if_statement", "conditional_expression", "match_statement"];

// Empty: `self.helper()` captures receiver `self`, which the resolver's
// this/self logic already maps to the containing class.
const BUILTIN_RECEIVERS: &[&str] = &[];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Python,
        tree_sitter_python::LANGUAGE.into(),
        &["py"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        TYPE_KINDS,
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::DottedPackage,
        BUILTIN_RECEIVERS,
    )
}

const STRING_KINDS: &[&str] = &["string"];
