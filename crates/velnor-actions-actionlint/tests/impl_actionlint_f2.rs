//! F2 closure cases for GEN-2.11, GEN-2.14, GEN-2.16.
use velnor_actions_actionlint::{
    ACTIONLINT_VERSION, ActionlintConfigInput, ActionlintToolchain, RUNNER_LABEL_BRIDGE,
    render_actionlint_yaml,
};

/// GEN-2.11: rendered bytes never carry init/fetch machinery.
#[test]
fn gen_2_11_rendered_bytes_never_carry_init_or_fetch() {
    let input = ActionlintConfigInput::new("0.1.0")
        .with_runner_label(RUNNER_LABEL_BRIDGE)
        .with_config_variable("ALPHA")
        .with_workflow_path(".github/workflows/ci.yml");
    let output = render_actionlint_yaml(&input).expect("renders");
    for token in ["init-config", "fetch", "http://", "https://"] {
        assert!(
            !output.yaml.contains(token),
            "rendered bytes must not contain {token:?}"
        );
    }
    let again = render_actionlint_yaml(&input).expect("renders");
    assert_eq!(output.yaml, again.yaml);
}

/// GEN-2.14: bulk variables emit the exact declared set, sorted and deduped.
#[test]
fn gen_2_14_bulk_variables_emit_exact_declared_set() {
    let input = ActionlintConfigInput::new("0.1.0")
        .with_runner_label(RUNNER_LABEL_BRIDGE)
        .with_config_variables(["ZULU", "ALPHA", "ZULU", "MID"]);
    let output = render_actionlint_yaml(&input).expect("renders");
    let mut in_variables = false;
    let mut rendered = Vec::new();
    for line in output.yaml.lines() {
        if line == "config-variables:" {
            in_variables = true;
            continue;
        }
        if in_variables {
            if let Some(name) = line.strip_prefix("  - ") {
                rendered.push(name.to_owned());
            } else {
                break;
            }
        }
    }
    assert_eq!(
        rendered,
        vec!["ALPHA".to_owned(), "MID".to_owned(), "ZULU".to_owned()]
    );
    let empty_input = ActionlintConfigInput::new("0.1.0").with_runner_label(RUNNER_LABEL_BRIDGE);
    let empty = render_actionlint_yaml(&empty_input).expect("renders");
    assert!(empty.yaml.contains("config-variables: []"));
    assert!(!output.yaml.contains("runs-on"));
}

/// GEN-2.16: one pinned-binary argv lints staged config plus all workflows.
#[test]
fn gen_2_16_staged_argv_lints_config_and_workflows_together() {
    let toolchain = ActionlintToolchain::pinned();
    assert_eq!(toolchain.version(), ACTIONLINT_VERSION);
    let workflows = vec![
        ".github/workflows/a.yml".to_owned(),
        ".github/workflows/ci.yml".to_owned(),
    ];
    let argv = ActionlintToolchain::staged_lint_argv(".github/actionlint.yaml", &workflows);
    assert_eq!(
        argv,
        vec![
            "-no-color".to_owned(),
            "-oneline".to_owned(),
            "-config-file".to_owned(),
            ".github/actionlint.yaml".to_owned(),
            ".github/workflows/a.yml".to_owned(),
            ".github/workflows/ci.yml".to_owned(),
        ]
    );
    assert_eq!(
        argv.iter()
            .filter(|arg| arg.as_str() == "-config-file")
            .count(),
        1
    );
    let config_pos = argv
        .iter()
        .position(|arg| arg.as_str() == ".github/actionlint.yaml")
        .expect("config present");
    for workflow in &workflows {
        let pos = argv
            .iter()
            .position(|arg| arg == workflow)
            .expect("workflow");
        assert!(pos > config_pos, "workflow after config: {workflow}");
    }
}
