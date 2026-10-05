//! Tofu provider-cache step cases: save shape, path allowlist, layer admission.
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::steps::{
    TOOLS_RESTORE_USES, TOOLS_SAVE_USES, cache_action_step,
};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_RESTORE_NAME, TOFU_PROVIDERS_SAVE_NAME,
    TOFU_PROVIDERS_SAVE_USES, tofu_providers_path_ok, tofu_providers_save_step,
};

const KEY: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";
const PATH: &str = "${{ runner.temp }}/velnor/tofu-cache/root-0123456789ab";

#[test]
fn provider_save_step_shape_and_pin_parity() -> Result<(), RenderError> {
    assert_eq!(
        TOFU_PROVIDER_CACHE_BASE_EXPR,
        "${{ runner.temp }}/velnor/tofu-cache"
    );
    assert_eq!(TOFU_PROVIDERS_SAVE_USES, TOOLS_SAVE_USES);
    let save = tofu_providers_save_step(KEY, PATH)?;
    assert_eq!(save.name, TOFU_PROVIDERS_SAVE_NAME);
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOOLS_SAVE_USES);
    assert_eq!(with.get("key").map(String::as_str), Some(KEY));
    assert_eq!(with.get("path").map(String::as_str), Some(PATH));
    assert!(
        !with.contains_key("restore-keys"),
        "saves carry no restore keys"
    );
    Ok(())
}

#[test]
fn provider_layer_admits_exact_keys_without_restore_prefix() -> Result<(), RenderError> {
    let restore = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tofu-providers",
        KEY,
        &[],
        &[PATH.to_owned()],
    )?;
    let StepKind::Action { with, .. } = &restore.kind else {
        panic!("restore must be an action step");
    };
    assert_eq!(with.get("key").map(String::as_str), Some(KEY));
    assert_eq!(
        with.get("restore-keys").map(String::as_str),
        Some(""),
        "L2 exact-key restore carries no prefix"
    );
    assert_eq!(TOFU_PROVIDERS_RESTORE_NAME, "Restore Tofu providers");
    assert!(
        cache_action_step(
            true,
            TOOLS_RESTORE_USES,
            "mbx",
            KEY,
            &[],
            &[PATH.to_owned()]
        )
        .is_err(),
        "the new arm widens nothing else"
    );
    Ok(())
}

#[test]
fn provider_paths_stay_under_the_owned_base() {
    assert!(tofu_providers_path_ok(PATH));
    assert!(tofu_providers_path_ok(
        "${{ runner.temp }}/velnor/tofu-cache/stacks-vpc-abcdef012345"
    ));
    for bad in [
        TOFU_PROVIDER_CACHE_BASE_EXPR,
        "${{ runner.temp }}/velnor/tofu-cache/",
        "${{ runner.temp }}/velnor/tofu-cache/a/b",
        "${{ runner.temp }}/velnor/tofu-cache/../evil",
        "${{ runner.temp }}/velnor/tofu-cache/root-x credentials",
        "${{ runner.temp }}/velnor/tofu-cache/credentials-root-0123456789ab",
        "${{ runner.temp }}/velnor/tofu-cache/root-*.hcl",
        "${{ runner.temp }}/velnor/tofu-data/root-0123456789ab",
        "$RUNNER_TEMP/velnor/tofu-cache/root-0123456789ab",
        "",
    ] {
        assert!(!tofu_providers_path_ok(bad), "{bad:?} must fail closed");
    }
}
