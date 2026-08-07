use super::LangSpec;
use crate::types::{ImportStyle, Lang};

// Validated against tests/fixtures/java/*.java (see tests/java_test.rs).
//
// Notes:
// - `method_invocation` is matched by a single pattern with an optional
//   `object:` capture. Two separate patterns (with/without object) would both
//   match receiver calls and the extractor's dedupe keeps whichever match
//   arrives first — which could drop `@call.recv`. One pattern avoids that.
// - Plain imports anchor the `scoped_identifier` as the last named child so
//   the wildcard form (`import java.util.*;`, which has a trailing
//   `(asterisk)`) doesn't bind its final package segment as an import name.
// - `package` accepts both multi-segment (`scoped_identifier`) and
//   single-segment (`identifier`) declarations.
// - A record's compact constructor (`record R(int x) { R { ... } }`) has no
//   parameter list of its own; the pattern reaches up to the enclosing
//   `record_declaration` so the record header supplies `@func.params`.
// - The class-level annotation patterns deliberately capture NEITHER
//   `@deco.type` (that would route it to `ExtractedFile.type_decorations`,
//   which endpoint detection never sees) NOR `@func.def`: with no same-match
//   definition the extractor falls back to nearest-following-function
//   association, attaching the annotation to the class's first method — the
//   endpoint pre-pass recovers the prefix from there via `containing_type`
//   (same ride-along trick the TS query uses for NestJS `@Controller`).
//   Known limit: a mapped class with no methods leaks the annotation to the
//   next function in the file. Gated to the route-shaping names —
//   `RequestMapping` (Spring prefix), `Path` (JAX-RS prefix) on classes;
//   those two plus the declarative-client markers `FeignClient` /
//   `RegisterRestClient` on interfaces (interface methods are captured by the
//   plain method_declaration patterns, so Feign/rest-client interfaces
//   otherwise read as servers) — so ordinary class annotations (`@Entity`)
//   don't pollute the decoration stream or set `has_decorations` on
//   unrelated methods. `RegisterRestClient` additionally needs a
//   marker_annotation twin (it is commonly argument-less).
// - DI type captures (shapes verified via probe dump):
//   - `field_declaration` keeps annotations (`@Autowired`) inside `modifiers`,
//     so one pattern covers plain and annotated fields alike. Generic fields
//     (`List<Book> books`) capture the container's `type_identifier` (`List`)
//     — `clean_type` rejects the full `generic_type` text, and no container
//     type is ever indexed, so this is harmless; the type-argument is NOT
//     captured (it would wrongly resolve `this.books.add()` to `Book`).
//     Primitive fields (`int count`, kind `integral_type`) don't match and are
//     skipped by design. Qualified fields (`com.acme.Mailer m`, kind
//     `scoped_type_identifier`, probe-verified) capture the WHOLE scoped node
//     — its text is dot-joined with no whitespace, so clean_type keeps the
//     last segment (`Mailer`).
//   - `formal_parameter type:`/`name:` are named fields; catch params are a
//     different kind (`catch_formal_parameter`) and stay out. Generic-typed
//     params and locals (`List<String> tags`) capture the container's
//     `type_identifier` exactly like the generic field pattern (a
//     `java.util.List<T>` generic wraps a `scoped_type_identifier` instead
//     and stays out, same as everywhere else).
//   - `local_variable_declaration` carries the declared type; `var` parses as
//     a `type_identifier` with text "var", so the declared-type pattern is
//     gated with `#not-eq?` and a value-side pattern recovers the type from
//     `var x = new Foo()` instead.
//   - Hierarchy: `superclass:` is a named field holding a `(superclass
//     (type_identifier))` node; class/record `implements` is `interfaces:
//     (super_interfaces (type_list ...))`; interface `extends` is an unnamed
//     `(extends_interfaces (type_list ...))` child. A multi-entry `type_list`
//     yields one query match per base, i.e. one `@hier` edge each.
const QUERY: &str = r#"
(method_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  (modifiers
    (annotation
      name: (identifier) @deco.name
      arguments: (annotation_argument_list) @deco.args) @deco)
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(method_declaration
  (modifiers
    (marker_annotation
      name: (identifier) @deco.name) @deco)
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

((class_declaration
  (modifiers
    (annotation
      name: (identifier) @deco.name
      arguments: (annotation_argument_list) @deco.args) @deco))
  (#any-of? @deco.name "RequestMapping" "Path"))

((interface_declaration
  (modifiers
    (annotation
      name: (identifier) @deco.name
      arguments: (annotation_argument_list) @deco.args) @deco))
  (#any-of? @deco.name "RequestMapping" "Path" "FeignClient" "RegisterRestClient"))

((interface_declaration
  (modifiers
    (marker_annotation
      name: (identifier) @deco.name) @deco))
  (#eq? @deco.name "RegisterRestClient"))

(constructor_declaration
  name: (identifier) @func.name
  parameters: (formal_parameters) @func.params) @func.def

(record_declaration
  parameters: (formal_parameters) @func.params
  body: (class_body
    (compact_constructor_declaration
      name: (identifier) @func.name) @func.def))

(method_invocation
  object: (_)? @call.recv
  name: (identifier) @call.name
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (type_identifier) @call.name
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (generic_type (type_identifier) @call.name)
  arguments: (argument_list) @call.args) @call

(object_creation_expression
  type: (scoped_type_identifier (type_identifier) @call.name .)
  arguments: (argument_list) @call.args) @call

(import_declaration
  (scoped_identifier
    name: (identifier) @import.name) @import.path .) @import

(import_declaration
  (scoped_identifier) @import.path
  (asterisk)) @import

(package_declaration (scoped_identifier) @package.name)

(package_declaration (identifier) @package.name)

(field_declaration
  declarator: (variable_declarator
    name: (identifier) @const.name
    value: (string_literal) @const.value))

(return_statement (string_literal) @ret.str)

(field_declaration
  type: (type_identifier) @field.type
  declarator: (variable_declarator
    name: (identifier) @field.name))

(field_declaration
  type: (generic_type (type_identifier) @field.type)
  declarator: (variable_declarator
    name: (identifier) @field.name))

(field_declaration
  type: (scoped_type_identifier) @field.type
  declarator: (variable_declarator
    name: (identifier) @field.name))

(formal_parameter
  type: (type_identifier) @local.type
  name: (identifier) @local.name)

(formal_parameter
  type: (generic_type (type_identifier) @local.type)
  name: (identifier) @local.name)

((local_variable_declaration
  type: (type_identifier) @local.type
  declarator: (variable_declarator
    name: (identifier) @local.name))
  (#not-eq? @local.type "var"))

(local_variable_declaration
  type: (generic_type (type_identifier) @local.type)
  declarator: (variable_declarator
    name: (identifier) @local.name))

(local_variable_declaration
  declarator: (variable_declarator
    name: (identifier) @local.name
    value: (object_creation_expression
      type: (type_identifier) @local.type)))

(class_declaration
  name: (identifier) @hier.type
  superclass: (superclass (type_identifier) @hier.base))

(class_declaration
  name: (identifier) @hier.type
  interfaces: (super_interfaces (type_list (type_identifier) @hier.base)))

(interface_declaration
  name: (identifier) @hier.type
  (extends_interfaces (type_list (type_identifier) @hier.base)))

(record_declaration
  name: (identifier) @hier.type
  interfaces: (super_interfaces (type_list (type_identifier) @hier.base)))
"#;

const IDENTIFIER_KINDS: &[&str] = &["identifier", "type_identifier"];

const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "name"),
    ("interface_declaration", "name"),
    ("enum_declaration", "name"),
    ("record_declaration", "name"),
    ("annotation_type_declaration", "name"),
];

const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "enhanced_for_statement",
    "while_statement",
    "do_statement",
];

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "switch_expression",
    "ternary_expression",
    "catch_clause",
];

const BUILTIN_RECEIVERS: &[&str] = &["System", "Math", "Objects", "Arrays", "Collections"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Java,
        tree_sitter_java::LANGUAGE.into(),
        &["java"],
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

const STRING_KINDS: &[&str] = &["string_literal"];
