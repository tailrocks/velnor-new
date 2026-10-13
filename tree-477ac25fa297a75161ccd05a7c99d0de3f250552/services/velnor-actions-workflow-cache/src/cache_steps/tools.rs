//! Tools-cache key namespace, pins, and restore/save templates.
//!
//! Declared via `#[path]` from `cache_steps.rs` (no `lib.rs` edit;
//! split under the 400-line size gate, bodies byte-identical).

use velnor_actions_contract_workflow::{Step, StepRole};

use velnor_actions_workflow_steps::RenderError;

/// Pinned `actions/cache/restore` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_RESTORE_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Display name of the tools restore step.
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
/// Display name of the tools save step.
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
/// Sole tools-cache path: the default mise data dir.
pub const TOOLS_CACHE_PATH: &str = "~/.local/share/mise";
/// Tools-cache key namespace.
pub const TOOLS_KEY_PREFIX: &str = "mise-tools-v1";
/// Tool files hashed into the tools key (literal names, never globs).
const TOOLS_KEY_FILES: &str = "'mise.toml','.mise.toml','mise.lock','.mise.lock','.tool-versions'";

/// Tools-cache key: target, mise, generator, job, plus tool-file hash.
///
/// Static segments invalidate exactly when pins change; the trailing
/// `hashFiles` over literal tool-file names (no workspace walk, no
/// ELOOP) churns the key when tool files change. No spaces: the cache
/// action rejects them.
/// # Errors
pub fn tools_cache_key(
    target: &str,
    mise_version: &str,
    generator_version: &str,
    job_id: &str,
) -> Result<String, RenderError> {
    if !velnor_actions_contract_release::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_target:{target}"
        )));
    }
    for (label, value) in [
        ("mise", mise_version),
        ("generator", generator_version),
        ("job", job_id),
    ] {
        if !is_key_segment(value) {
            return Err(RenderError::BadCommand(format!(
                "bad_cache_key_{label}:{value}"
            )));
        }
    }
    Ok(format!(
        "{TOOLS_KEY_PREFIX}-{target}-{mise_version}-{generator_version}-{job_id}-${{{{hashFiles({TOOLS_KEY_FILES})}}}}"
    ))
}

/// Prefix restore key: same pins and job, any tool-file hash.
fn tools_restore_keys(key: &str) -> Vec<String> {
    vec![tools_key_prefix_of(key)]
}

/// Key minus its trailing hash expression, kept as a prefix.
///
/// Splits on the expression marker (never on `-`: file names inside
/// the hash carry dashes); marker-less keys degrade to a full prefix.
fn tools_key_prefix_of(key: &str) -> String {
    key.split_once("${{")
        .map_or_else(|| format!("{key}-"), |(head, _)| head.to_owned())
}

/// Tools restore step over the pinned restore action.
/// # Errors
pub fn tools_restore_step(key: &str) -> Result<Step, RenderError> {
    let restore_keys = tools_restore_keys(key);
    let step = super::cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        key,
        &restore_keys,
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    rename_step(step, TOOLS_RESTORE_NAME)
}

/// Tools save step over the pinned save action.
/// # Errors
pub fn tools_save_step(key: &str) -> Result<Step, RenderError> {
    let step = super::cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "tools",
        key,
        &[],
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    let mut step = rename_step(step, TOOLS_SAVE_NAME)?;
    step.role = Some(StepRole::ToolsCacheSave);
    Ok(step)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    velnor_actions_workflow_steps::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}

// P08: the manual `ensure_tools_cache` wrapper is removed. Tools restore
// through the Mise action's built-in cache (`cache_p08::ensure_setup_p08`)
// and save through explicit `Save Mise tools` steps on the elected writer
// per key (`cache_elect::elect_mise_cache_writers`); the action's built-in
// save is unreachable with `install: false`. `tools_restore_step` stays
// unit-test-only (restores are never manual); the `mise-tools-v1` key
// namespace is retired; runtime-identity suffixes are part of the key.

/// Key segments: nonempty alphanumerics plus `.-_`, never `latest`.
fn is_key_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
