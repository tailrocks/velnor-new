//! Lint-tool pin cases.
use velnor_actions_actionlint::{
    ACTIONLINT_VERSION, ActionlintError, ActionlintToolchain, ShellcheckToolchain,
    WorkflowLintTools, ZizmorToolchain,
};

#[test]
fn single_pinned_actionlint_identity_for_config_and_workflows() {
    let tools = WorkflowLintTools::new("1.9.0").expect("valid zizmor pin");
    assert_eq!(tools.actionlint.version(), ACTIONLINT_VERSION);
    assert_eq!(
        tools.actionlint.mise_tool_spec(),
        format!("actionlint@{ACTIONLINT_VERSION}"),
        "one pinned binary lints both the staged config and every workflow"
    );
    assert_eq!(
        tools.mise_tool_specs()[0],
        tools.actionlint.mise_tool_spec()
    );
}

#[test]
fn pinned_tool_specs_are_exact() {
    assert_eq!(
        ActionlintToolchain::pinned().mise_tool_spec(),
        "actionlint@1.7.12"
    );
    assert_eq!(
        ShellcheckToolchain::pinned().mise_tool_spec(),
        "shellcheck@0.11.0"
    );
    assert_eq!(ActionlintToolchain::default().version(), "1.7.12");
    assert_eq!(ShellcheckToolchain::default().version(), "0.11.0");
}

#[test]
fn zizmor_requires_explicit_exact_version() {
    assert!(ZizmorToolchain::new("1.9.0").is_ok());
    for version in ["v1.9.0", "latest", "1.9", "1.9.0-beta", ""] {
        assert!(
            matches!(
                ZizmorToolchain::new(version),
                Err(ActionlintError::InvalidToolVersion { .. })
            ),
            "version must be rejected: {version}"
        );
    }
}

#[test]
fn lint_tool_specs_have_fixed_order() {
    let tools = WorkflowLintTools::new("1.9.0");
    assert!(tools.is_ok());
    if let Ok(tools) = tools {
        assert_eq!(
            tools.mise_tool_specs(),
            vec![
                "actionlint@1.7.12".to_owned(),
                "shellcheck@0.11.0".to_owned(),
                "zizmor@1.9.0".to_owned(),
            ]
        );
    }
    assert!(matches!(
        WorkflowLintTools::new("latest"),
        Err(ActionlintError::InvalidToolVersion { .. })
    ));
}
