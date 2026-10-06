//! Helper outcome expressions grant no arbitrary step-context authority.

use super::check_env_value;

#[test]
fn helper_outcome_requires_exact_typed_helper_step() {
    let expression = "${{ steps.velnor-helper-m-0123456789abcdef.outcome }}";
    assert!(check_env_value("VELNOR_HELPER_OUTCOME", expression).is_ok());
    assert!(check_env_value("OTHER", expression).is_err());
    assert!(check_env_value("VELNOR_ACTION_OUTCOME", expression).is_err());
    for inner in [
        "steps.other.outcome",
        "steps.velnor-action-m-0123456789abcdef.outcome",
        "steps.velnor-helper-m-xyz.outcome",
        "steps.velnor-helper-m-0123456789ABCDEF.outcome",
        "steps.velnor-helper-m-0123456789abcdef-export.outcome",
        "steps.velnor-helper-m-0123456789abcdef.outputs.outcome",
        "steps.velnor-helper-m-0123456789abcdef.outcome || 'success'",
    ] {
        assert!(check_env_value("VELNOR_HELPER_OUTCOME", &format!("${{{{ {inner} }}}}")).is_err());
    }
}

#[test]
fn helper_outcome_rejects_literals_and_interpolation_suffixes() {
    for expression in [
        "success",
        "${{ matrix.outcome }}",
        "success${{ steps.velnor-helper-m-0123456789abcdef.outcome }}",
        "${{ steps.velnor-helper-m-0123456789abcdef.outcome }}${{ runner.temp }}",
    ] {
        assert!(check_env_value("VELNOR_HELPER_OUTCOME", expression).is_err());
    }
}

#[test]
fn oidc_environment_expressions_require_the_exact_named_binding() {
    for key in [
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    ] {
        let value = format!("${{{{ env.{key} }}}}");
        assert!(super::check_env_value(key, &value).is_ok());
        assert!(super::check_env_value("OTHER", &value).is_err());
        assert!(super::check_env_value(key, &format!("prefix{value}")).is_err());
        assert!(super::check_env_value(key, "${{ env.OTHER }}").is_err());
    }
}
