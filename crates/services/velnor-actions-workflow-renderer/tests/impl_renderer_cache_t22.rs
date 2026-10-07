//! T22 never-archive exclusions in rendered cache steps.
use velnor_actions_workflow_renderer::cache_steps::{
    NEVER_ARCHIVE_MARKERS, TOOLS_RESTORE_USES, cache_action_step, is_never_archive_path,
};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_CACHE_BASE_EXPR, tofu_providers_path_ok,
};
use velnor_actions_workflow_steps::RenderError;

const HOME: &str = "${{ runner.temp }}/velnor/cargo";
const KEY: &str = "velnor-v1-sources-trusted-compat-snapshot";

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
fn provider_paths_reject_never_archive_paths() {
    let good = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab");
    assert!(tofu_providers_path_ok(&good));
    for bad in [
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab.tfstate"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/plan.tfplan"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/credentials-root-0123456789ab"),
    ] {
        assert!(!tofu_providers_path_ok(&bad), "must reject {bad}");
    }
}
