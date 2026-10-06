//! Action outcomes admit only the fixed closed adapter expression.

use super::check_env_value;

#[test]
fn action_outcome_is_bound_to_its_private_environment_key() {
    let value = "${{ steps.velnor-action-m-0123456789abcdef.outcome }}";
    assert!(check_env_value("VELNOR_ACTION_OUTCOME", value).is_ok());
    for key in ["OTHER", "VELNOR_TASK_ID", "VELNOR_ACTION_ID"] {
        assert!(check_env_value(key, value).is_err(), "{key}");
    }
}

#[test]
fn action_outcome_rejects_unreviewed_steps_outputs_and_logic() {
    for inner in [
        "steps.other.outcome",
        "steps.velnor-action-.outcome",
        "steps.velnor-action-m-.outcome",
        "steps.velnor-action-m-xyz.outcome",
        "steps.velnor-action-m-0123456789ABCDEF.outcome",
        "steps.velnor-action-m-0123456789abcdef-export.outcome",
        "steps.velnor-action-m-0123456789abcdef.conclusion",
        "steps.velnor-action-m-0123456789abcdef.outputs.outcome",
        "steps.velnor-action-m-0123456789abcdef.outcome || 'success'",
        "steps.velnor-action-m-0123456789abcdef.outcome == 'success'",
        "steps.velnor-action-m.0123456789abcdef.outcome",
        "steps.velnor-action-m-0123456789abcdef.outcome }} ${{ github.token",
        "secrets.GITHUB_TOKEN || steps.velnor-action-m-0123456789abcdef.outcome",
    ] {
        assert!(
            check_env_value("VELNOR_ACTION_OUTCOME", &format!("${{{{ {inner} }}}}")).is_err(),
            "{inner}"
        );
    }
}

#[test]
fn action_outcome_rejects_literals_and_multiple_expression_spans() {
    let outcome = "${{ steps.velnor-action-m-0123456789abcdef.outcome }}";
    for value in [
        "success".to_owned(),
        format!("success{outcome}"),
        format!("{outcome}success"),
        format!("{outcome} ${{{{ github.token }}}}"),
        format!("{outcome}${{{{ runner.temp }}}}"),
        "${{ github.token }}".to_owned(),
        "${{ matrix.outcome }}".to_owned(),
    ] {
        assert!(
            check_env_value("VELNOR_ACTION_OUTCOME", &value).is_err(),
            "{value}"
        );
    }
}
