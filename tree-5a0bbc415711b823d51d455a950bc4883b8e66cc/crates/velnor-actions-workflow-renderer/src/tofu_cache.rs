//! Tofu provider-cache step templates: saves plus the owned-path allowlist.
//!
//! Restore steps arrive from the orchestrator (per-root keys need the
//! catalog tofu pin); saves append post-hoc through writer election
//! ([`elect_tofu_provider_savers`](crate::cache_p08::elect_tofu_provider_savers)).
//! Both archive exactly one job-private plugin-cache dir, never the
//! data dir beside it.

use velnor_actions_contract::Step;

use crate::{RenderError, cache_steps};

/// Owned plugin-cache base (expression form for `path:`/`env:`).
///
/// The contract's `$RUNNER_TEMP/velnor/tofu-cache/<slug>` shell
/// spelling names this same dir for `run:` scripts; cache paths and
/// step env carry the expression form (GitHub expands no `$VAR`
/// there).
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";
/// Display name of the provider restore step.
pub const TOFU_PROVIDERS_RESTORE_NAME: &str = "Restore Tofu providers";
/// Display name of the provider save step.
pub const TOFU_PROVIDERS_SAVE_NAME: &str = "Save Tofu providers";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
///
/// Same qualified pin as the tools save; a pin bump moves both.
pub const TOFU_PROVIDERS_SAVE_USES: &str =
    "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

/// Provider save step over the pinned save action.
///
/// The key and path read back from the elected restore; the push
/// gate arrives from the election caller, never here.
/// # Errors
pub fn tofu_providers_save_step(key: &str, path: &str) -> Result<Step, RenderError> {
    let step = cache_steps::cache_action_step(
        false,
        TOFU_PROVIDERS_SAVE_USES,
        "tofu-providers",
        key,
        &[],
        &[path.to_owned()],
    )?;
    rename_step(step, TOFU_PROVIDERS_SAVE_NAME)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}

/// True for exactly one owned plugin-cache dir under the base.
///
/// Single path segment, slug charset only, no traversal, no
/// never-archive names (state, plans, credentials), never the bare
/// base: the data dir beside it (`tofu-data`) and every foreign tree
/// stay out.
#[must_use]
pub fn tofu_providers_path_ok(path: &str) -> bool {
    if path.contains("..") || crate::cache_steps::is_never_archive_path(path) {
        return false;
    }
    let Some(slug) = path.strip_prefix(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/")) else {
        return false;
    };
    !slug.is_empty()
        && !slug.contains('/')
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
