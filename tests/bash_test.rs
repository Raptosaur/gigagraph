mod common;

use common::*;

#[test]
fn extracts_bash_library_functions() {
    let file = extract_fixture("sh", "bash/lib.sh");
    let names: Vec<&str> = file.functions.iter().map(|f| f.name.as_str()).collect();

    // `name() {}`, `function name {}`, and `function name() {}` forms.
    for expected in ["log_info", "log_error", "count_matches", "current_branch"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    // Bash functions have no parameter list.
    assert_eq!(func(&file, "count_matches").param_count, 0);

    // External commands are calls attributed to the enclosing function.
    assert_calls(func(&file, "count_matches"), &["grep"]);
    assert_calls(func(&file, "current_branch"), &["git"]);

    // `. ./colors.sh` is an import, not a `.` call. It is lib.sh's only
    // toplevel statement, so no synthetic toplevel function appears at all.
    assert!(import_paths(&file).contains(&"./colors.sh"));
    assert!(!names.contains(&"(toplevel)"));
    assert!(file.functions.iter().all(|f| !has_call(f, ".")));
}

#[test]
fn extracts_bash_script_calls_and_imports() {
    let file = extract_fixture("sh", "bash/main.sh");

    // Command substitution, `if` condition, then/else bodies, loop body.
    assert_calls(
        func(&file, "deploy"),
        &["current_branch", "grep", "log_info", "log_error", "rsync"],
    );

    // `while` condition, redirected loop body, pipeline stages.
    assert_calls(
        func(&file, "report"),
        &["read", "echo", "awk", "sort", "uniq"],
    );

    // Toplevel script body: case-arm calls and a command substitution
    // inside a double-quoted string.
    let top = func(&file, "(toplevel)");
    assert_calls(
        top,
        &["set", "deploy", "report", "log_error", "count_matches"],
    );

    // `source` lines become imports (bare word and quoted string paths),
    // not `source` calls.
    assert!(!has_call(top, "source"));
    let paths = import_paths(&file);
    for p in ["./lib.sh", "./config.sh"] {
        assert!(paths.contains(&p), "missing import {p}; got {paths:?}");
    }

    // Control-flow and identifier features.
    let deploy = func(&file, "deploy");
    assert_eq!(deploy.features.get("flow:loops"), Some(&1));
    assert_eq!(deploy.features.get("flow:branches"), Some(&1));
    assert!(deploy.features.contains_key("id:rsync")); // command_name
    assert!(deploy.features.contains_key("id:branch")); // variable_name
}
