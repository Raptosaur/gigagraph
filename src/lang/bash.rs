use super::LangSpec;
use crate::types::{ImportStyle, Lang};

/// All three definition forms (`foo() {}`, `function foo {}`,
/// `function foo() {}`) parse as `function_definition` with a `word` name.
/// Bash functions have no parameter list, so there is no `@func.params`
/// capture and `param_count` is always 0.
///
/// Every command invocation is a call: `(command name: (command_name (word)))`
/// covers plain statements, pipeline stages, `if`/`while` conditions and
/// command substitutions `$(...)` alike, since patterns match anywhere in the
/// tree. Command arguments are individual `argument:` fields on the `command`
/// node — there is no argument-list node to capture as `@call.args`, so
/// `arg_count` stays 0.
///
/// `source ./x.sh` and `. ./x.sh` are commands too; the `#any-of?` patterns
/// turn them into imports (first argument = path, bare word or quoted string)
/// while the `#not-any-of?` guard on the call pattern keeps them from also
/// being emitted as `source`/`.` calls. The `.` anchor pins `@import.path` to
/// the argument immediately following the command name.
///
/// Bats (`@test "name" { ... }`) is not bash syntax: tree-sitter-bash parses
/// the header as a plain `command` named `@test` whose arguments are the
/// description string and a bare `{`, and the body statements land as
/// *siblings*, not children. So the `@test` pattern below yields a function
/// named after the description that spans the header line only — enough for
/// test discovery (`list_tests`) and for `file_overview`, but the body's calls
/// are attributed to the file's top-level function rather than to the case.
/// The `#not-any-of?` guard on the call pattern keeps `@test` from also being
/// emitted as a call to a command named `@test`.
const QUERY: &str = r#"
(function_definition
  name: (word) @func.name) @func.def

(command
  name: (command_name (word) @_bats)
  argument: (string (string_content) @func.name)
  (#eq? @_bats "@test")) @func.def

(command
  name: (command_name (word) @call.name)
  (#not-any-of? @call.name "source" "." "@test")) @call

(command
  name: (command_name (word) @_src)
  .
  argument: (word) @import.path
  (#any-of? @_src "source" ".")) @import

(command
  name: (command_name (word) @_src)
  .
  argument: (string (string_content) @import.path)
  (#any-of? @_src "source" ".")) @import
"#;

/// `command_name` (not bare `word`) so command names become `id:` features
/// without flooding the bag with every argument word.
const IDENTIFIER_KINDS: &[&str] = &["variable_name", "command_name"];

/// `until` loops parse as `while_statement` in tree-sitter-bash.
const LOOP_KINDS: &[&str] = &["for_statement", "c_style_for_statement", "while_statement"];

const BRANCH_KINDS: &[&str] = &["if_statement", "case_statement"];

pub fn spec() -> LangSpec {
    LangSpec::new(
        Lang::Bash,
        tree_sitter_bash::LANGUAGE.into(),
        &["sh", "bash", "bats"],
        QUERY,
        IDENTIFIER_KINDS,
        STRING_KINDS,
        &[],
        LOOP_KINDS,
        BRANCH_KINDS,
        ImportStyle::PathLike,
        &[],
    )
}

const STRING_KINDS: &[&str] = &["string", "raw_string", "word", "concatenation"];
