//! `$/` actionlint gap: hosted bytes stay untouched; shared lanes gain one ignore.
use velnor_actions_actionlint::{
    ActionlintConfigInput, ActionlintError, RUNNER_LABEL_BRIDGE, SELF_REPO_ACTION_IGNORE,
    append_self_repo_action_gap, render_actionlint_yaml,
};

fn base() -> Result<String, ActionlintError> {
    let input = ActionlintConfigInput::new("0.1.0").with_runner_label(RUNNER_LABEL_BRIDGE);
    Ok(render_actionlint_yaml(&input)?.yaml)
}

#[test]
fn hosted_bytes_stay_unchanged() -> Result<(), ActionlintError> {
    let yaml = base()?;
    let again = append_self_repo_action_gap(
        &yaml,
        &[(".github/workflows/ci.yml", "uses: actions/checkout@v4\n")],
    )?;
    assert_eq!(again, yaml);
    assert!(!again.contains("paths:"));
    Ok(())
}

#[test]
fn shared_lane_gains_only_the_ref_gap() -> Result<(), ActionlintError> {
    let yaml = base()?;
    let ci = "jobs:\n  a:\n    steps:\n      - uses: $/.github/actions/rust-0\n";
    let out = append_self_repo_action_gap(&yaml, &[(".github/workflows/ci.yml", ci)])?;
    let gap = format!(
        "paths:\n  .github/workflows/ci.yml:\n    ignore:\n      - '{SELF_REPO_ACTION_IGNORE}'\n"
    );
    assert!(out.ends_with(&gap), "{out}");
    assert_eq!(out.matches("ignore:").count(), 1);
    assert!(!out.contains("shellcheck"));
    Ok(())
}

#[test]
fn marker_outside_a_workflow_is_rejected() -> Result<(), ActionlintError> {
    let yaml = base()?;
    let err = append_self_repo_action_gap(
        &yaml,
        &[(
            ".github/actions/rust-0/action.yml",
            "uses: $/.github/actions/rust-0\n",
        )],
    );
    assert!(matches!(
        err,
        Err(ActionlintError::InvalidWorkflowPath { .. })
    ));
    Ok(())
}
