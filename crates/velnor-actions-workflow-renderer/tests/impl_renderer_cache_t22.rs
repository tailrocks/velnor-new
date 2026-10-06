//! T22 never-archive exclusions in rendered cache steps.
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::steps::{
    NEVER_ARCHIVE_MARKERS, TOOLS_RESTORE_USES, cache_action_step, is_never_archive_path,
};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_CACHE_BASE_EXPR, tofu_providers_save_step,
};

const HOME: &str = "${{ runner.temp }}/velnor/cargo";
const KEY: &str = "velnor-v1-sources-trusted-compat-snapshot";
const PROVIDER_KEY: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";

#[test]
fn never_archive_mirror_lists_state_plans_and_credentials() {
    assert_eq!(
        NEVER_ARCHIVE_MARKERS,
        ["credentials", ".tfstate", ".tfplan"]
    );
    for marker in NEVER_ARCHIVE_MARKERS {
        assert!(is_never_archive_path(&format!("/cache/x{marker}")));
    }
    for clean in [
        "/cache/registry/cache/serde-1.0.228.crate",
        "/cache/.crates.toml",
        "/cache/bin/cargo-nextest",
    ] {
        assert!(!is_never_archive_path(clean), "{clean} stays archivable");
    }
}

#[test]
fn sources_steps_reject_never_archive_paths() -> Result<(), RenderError> {
    let good = format!("{HOME}/registry/cache");
    cache_action_step(true, TOOLS_RESTORE_USES, "sources", KEY, &[], &[good])?;
    for bad in [
        format!("{HOME}/registry/cache/state.tfstate"),
        format!("{HOME}/registry/cache/state.tfstate.backup"),
        format!("{HOME}/registry/cache/plan.tfplan"),
        format!("{HOME}/registry/cache/credentials.toml"),
        format!("{HOME}/bin/evil.tfplan"),
    ] {
        assert!(
            cache_action_step(
                true,
                TOOLS_RESTORE_USES,
                "sources",
                KEY,
                &[],
                std::slice::from_ref(&bad)
            )
            .is_err(),
            "must reject {bad}"
        );
    }
    Ok(())
}

#[test]
fn provider_steps_reject_never_archive_paths() -> Result<(), RenderError> {
    let good = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab");
    tofu_providers_save_step(PROVIDER_KEY, &good)?;
    for bad in [
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab.tfstate"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/plan.tfplan"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/credentials-root-0123456789ab"),
    ] {
        assert!(
            tofu_providers_save_step(PROVIDER_KEY, &bad).is_err(),
            "must reject {bad}"
        );
    }
    Ok(())
}

#[test]
fn provider_save_carries_no_gate_itself() -> Result<(), RenderError> {
    let good = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab");
    let save = tofu_providers_save_step(PROVIDER_KEY, &good)?;
    assert!(
        save.condition.is_none(),
        "the push gate arrives from the election caller, never the template"
    );
    Ok(())
}
