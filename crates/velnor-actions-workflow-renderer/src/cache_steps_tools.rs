//! Tools-cache key namespace, pins, and restore/save templates.
//!
//! Declared via `#[path]` from `cache_steps.rs` (no `lib.rs` edit;
//! split under the 400-line size gate, bodies byte-identical).

use velnor_actions_contract::Step;

use crate::RenderError;

/// Pinned `actions/cache/restore` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_RESTORE_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Display name of the tools restore step.
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
/// Display name of the tools save step.
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
/// Owned Mise root, resolved by the runner before both cache operations.
pub const TOOLS_CACHE_PATH: &str = "${{ runner.temp }}/velnor/mise";
/// Stable restore output binding.
pub(crate) const TOOLS_RESTORE_ID: &str = "velnor-tools-cache";
/// One ordered payload definition used by both operations.
#[must_use]
pub fn tool_payload_paths() -> Vec<String> {
    velnor_actions_contract::ToolCacheDomain::Full.payload()
}
/// Accept only explicitly owned executable payload roots.
#[must_use]
pub(crate) fn is_tool_payload_path(path: &str) -> bool {
    use velnor_actions_contract::ToolCacheDomain;
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
    .into_iter()
    .any(|domain| domain.payload().iter().any(|owned| owned == path))
}
/// Tools restore step over the pinned restore action.
/// # Errors
pub fn tools_restore_step(key: &str) -> Result<Step, RenderError> {
    let restore_keys = vec![format!("{key}-snapshot-")];
    let lookup = format!("{key}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}");
    let mut step = super::cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        &lookup,
        &restore_keys,
        &tool_payload_paths(),
    )?;
    step.id = Some(
        velnor_actions_contract::StepId::new(TOOLS_RESTORE_ID).map_err(RenderError::Contract)?,
    );
    rename_step(step, TOOLS_RESTORE_NAME)
}

/// Tools save step over the pinned save action.
/// # Errors
pub fn tools_save_step(key: &str) -> Result<Step, RenderError> {
    let snapshot = format!(
        "{key}-snapshot-${{{{env.VELNOR_TOOLS_SNAPSHOT_DIGEST}}}}-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}"
    );
    let step = super::cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "tools",
        &snapshot,
        &[],
        &tool_payload_paths(),
    )?;
    rename_step(step, TOOLS_SAVE_NAME)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}
