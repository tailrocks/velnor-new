//! Tofu provider-cache step cases: save shape, path allowlist, layer admission.
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::steps::TOOLS_SAVE_USES;
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_KEY_OUTPUT_EXPR,
    TOFU_PROVIDERS_PATH_OUTPUT_EXPR, TOFU_PROVIDERS_RESTORE_NAME, TOFU_PROVIDERS_SAVE_NAME,
    TOFU_PROVIDERS_SAVE_USES, tofu_providers_path_ok, tofu_providers_save_step,
};

#[test]
fn provider_save_step_shape_and_pin_parity() -> Result<(), RenderError> {
    assert_eq!(
        TOFU_PROVIDER_CACHE_BASE_EXPR,
        "${{ runner.temp }}/velnor/tofu-cache"
    );
    assert_eq!(TOFU_PROVIDERS_SAVE_USES, TOOLS_SAVE_USES);
    let save = tofu_providers_save_step()?;
    assert_eq!(save.name, TOFU_PROVIDERS_SAVE_NAME);
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOOLS_SAVE_USES);
    assert_eq!(
        with.get("key").map(String::as_str),
        Some(TOFU_PROVIDERS_KEY_OUTPUT_EXPR)
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(TOFU_PROVIDERS_PATH_OUTPUT_EXPR)
    );
    assert!(
        !with.contains_key("restore-keys"),
        "saves carry no restore keys"
    );
    Ok(())
}

#[test]
fn provider_restore_and_admission_share_one_typed_composite() {
    assert_eq!(
        TOFU_PROVIDER_ADMISSION_USES,
        "./.github/actions/tofu-provider-admission"
    );
    assert_eq!(TOFU_PROVIDERS_RESTORE_NAME, "Restore Tofu providers");
}

#[test]
fn provider_paths_stay_under_the_owned_base() {
    assert!(tofu_providers_path_ok(
        "${{ runner.temp }}/velnor/tofu-cache/b3-0000000000000000000000000000000000000000000000000000000000000000"
    ));
    assert!(tofu_providers_path_ok(
        "${{ runner.temp }}/velnor/tofu-cache/b3-1111111111111111111111111111111111111111111111111111111111111111"
    ));
    for bad in [
        TOFU_PROVIDER_CACHE_BASE_EXPR,
        "${{ runner.temp }}/velnor/tofu-cache/",
        "${{ runner.temp }}/velnor/tofu-cache/a/b",
        "${{ runner.temp }}/velnor/tofu-cache/../evil",
        "${{ runner.temp }}/velnor/tofu-cache/root-x credentials",
        "${{ runner.temp }}/velnor/tofu-cache/credentials-b3-0000000000000000000000000000000000000000000000000000000000000000",
        "${{ runner.temp }}/velnor/tofu-cache/root-*.hcl",
        "${{ runner.temp }}/velnor/tofu-data/b3-0000000000000000000000000000000000000000000000000000000000000000",
        "$RUNNER_TEMP/velnor/tofu-cache/b3-0000000000000000000000000000000000000000000000000000000000000000",
        "",
    ] {
        assert!(!tofu_providers_path_ok(bad), "{bad:?} must fail closed");
    }
}
