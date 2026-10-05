//! V2 tools-cache namespace, paths, and restore/save templates.
//!
//! Declared via `#[path]` from `cache_steps.rs` (no `lib.rs` edit;
//! split under the 400-line size gate, bodies byte-identical).

use velnor_actions_contract::{Step, StepRole};

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
/// Mise tool installation root.
pub const TOOLS_CACHE_PATH: &str = "~/.local/share/mise";
/// Exact V2 tool-payload paths shared by restore and save.
pub const TOOLS_CACHE_PATHS: [&str; 5] = [
    TOOLS_CACHE_PATH,
    "${{ runner.temp }}/velnor/rustup",
    "${{ runner.temp }}/velnor/cargo/.crates.toml",
    "${{ runner.temp }}/velnor/cargo/.crates2.json",
    "${{ runner.temp }}/velnor/cargo/bin",
];
/// Whether the V2 tools layer owns one of its exact payload paths.
pub(super) fn tools_cache_path_ok(path: &str) -> bool {
    TOOLS_CACHE_PATHS.contains(&path)
}

/// V2 tools restore/save step over the exact typed payload paths.
/// # Errors
pub(super) fn cache_step(
    restore: bool,
    key: &str,
    condition: Option<String>,
) -> Result<Step, RenderError> {
    let name = if restore {
        TOOLS_RESTORE_NAME
    } else {
        TOOLS_SAVE_NAME
    };
    let uses = if restore {
        TOOLS_RESTORE_USES
    } else {
        TOOLS_SAVE_USES
    };
    let mut step = super::cache_action_step(
        restore,
        uses,
        "tools",
        key,
        &[],
        &TOOLS_CACHE_PATHS.map(str::to_owned),
    )?;
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    step.role = Some(if restore {
        StepRole::ToolsCacheRestore
    } else {
        StepRole::ToolsCacheSave
    });
    step.condition = condition;
    Ok(step)
}
