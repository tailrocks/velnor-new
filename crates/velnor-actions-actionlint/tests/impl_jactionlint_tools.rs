//! Pinned lint tools and the one jactionlint argument contract.
use velnor_actions_actionlint::{
    JACTIONLINT_CONFIG_PATH, JACTIONLINT_VERSION, JactionlintToolchain, LINT_FILE_GLOBS,
    LINT_FLAGS, SHELLCHECK_VERSION, WorkflowLintTools, workflow_lint_script,
};

#[test]
fn pinned_tool_specs_are_exact_and_mirror_the_catalog() {
    // The adapter keeps its own copy of the pin; the freshness gate checks
    // it equals the inventory, so here it only has to be exact.
    let parts: Vec<&str> = JACTIONLINT_VERSION.split('.').collect();
    assert_eq!(parts.len(), 3, "{JACTIONLINT_VERSION}");
    assert!(parts.iter().all(|part| part.parse::<u32>().is_ok()));
    assert_eq!(
        WorkflowLintTools::pinned().mise_tool_specs(),
        vec![
            format!("jactionlint@{JACTIONLINT_VERSION}"),
            format!("shellcheck@{SHELLCHECK_VERSION}"),
        ]
    );
}

#[test]
fn every_run_fails_closed_offline_with_the_pinned_shellcheck() {
    assert_eq!(
        LINT_FLAGS,
        [
            "--strict-exit",
            "--no-online",
            "--shellcheck=shellcheck",
            "--pyflakes=",
            "--no-color",
        ]
    );
}

#[test]
fn staged_argv_names_the_config_then_flags_then_files() {
    let files = vec![
        ".github/workflows/ci.yml".to_owned(),
        ".github/actions/declared-task-0/action.yml".to_owned(),
    ];
    let argv = JactionlintToolchain::staged_lint_argv(JACTIONLINT_CONFIG_PATH, &files);
    let mut expected = vec![
        "--config-file".to_owned(),
        ".github/jactionlint.yaml".to_owned(),
        "--format".to_owned(),
        "oneline".to_owned(),
    ];
    expected.extend(LINT_FLAGS.iter().map(|flag| (*flag).to_owned()));
    expected.extend(files);
    assert_eq!(argv, expected);
}

#[test]
fn workflow_script_lints_exactly_the_hk_file_set() {
    assert_eq!(
        LINT_FILE_GLOBS,
        [
            ".github/workflows/*.y*ml",
            ".github/actions/**/action.y*ml",
            ".github/dependabot.y*ml",
        ]
    );
    let script = workflow_lint_script("mise exec jactionlint@2.3.0 -- jactionlint");
    for find in [
        "find .github/workflows -maxdepth 1 -type f -name \"*.y*ml\";",
        "find .github/actions -type f -name \"action.y*ml\";",
        "find .github -maxdepth 1 -type f -name \"dependabot.y*ml\";",
    ] {
        assert!(script.contains(find), "{find}: {script}");
    }
    assert!(script.contains("| LC_ALL=C sort)"), "{script}");
    assert!(
        script.contains("if [ \"${#files[@]}\" -eq 0 ]; then"),
        "{script}"
    );
    assert!(script.contains("exit 3"), "{script}");
    assert!(script.ends_with(&format!(
        "exec mise exec jactionlint@2.3.0 -- jactionlint --config-file .github/jactionlint.yaml --format github {} \"${{files[@]}}\"",
        LINT_FLAGS.join(" ")
    )));
    assert!(!script.contains("JACTIONLINT_NO_HINTS"), "{script}");
    assert!(!script.contains("docs/"), "{script}");
    assert!(!script.contains('\''), "{script}");
    assert!(!script.contains("$("), "{script}");
}
