//! Exact tools payload and restore/save templates.
//!
//! Declared via `#[path]` from `cache_steps.rs` (no `lib.rs` edit;
//! split under the 400-line size gate, bodies byte-identical).

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::RenderError;

/// Pinned `actions/cache/restore` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_RESTORE_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Display name of the tools restore step.
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
/// Name of the local cache-path validation step.
pub const WORKSPACE_CACHE_GUARD_NAME: &str = "Guard Velnor cache paths";
/// Display name of the tools save step.
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
/// Complete tools payload, including Mise, Rustup, and Cargo proxies.
pub const TOOLS_CACHE_PATHS: [&str; 3] = [
    ".velnor/cache/mise",
    ".velnor/cache/rustup",
    ".velnor/cache/cargo/bin",
];

/// Tools restore step over the pinned restore action.
/// # Errors
pub fn tools_restore_step(key: &str) -> Result<Step, RenderError> {
    let step = super::cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        key,
        &[],
        &TOOLS_CACHE_PATHS.map(str::to_owned),
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
        &TOOLS_CACHE_PATHS.map(str::to_owned),
    )?;
    rename_step(step, TOOLS_SAVE_NAME)
}

/// Validate private cache paths and exclude them when the job checks out code.
/// # Errors
pub fn workspace_cache_guard_step(has_checkout: bool) -> Result<Step, RenderError> {
    let mut commands = vec![
        "set -eu; workspace=${GITHUB_WORKSPACE:?};",
        "for relative in .velnor .velnor/cache .velnor/cache/mise .velnor/cache/rustup .velnor/cache/cargo .velnor/cache/cargo/bin .velnor/cache/cargo/registry .velnor/cache/cargo/registry/index .velnor/cache/cargo/registry/cache .velnor/cache/cargo/registry/src .velnor/cache/cargo/git .velnor/cache/cargo/git/db .velnor/cache/cargo/git/checkouts; do path=\"$workspace/$relative\"; if [ -L \"$path\" ]; then printf 'velnor_cache_symlink:%s\\n' \"$relative\" >&2; exit 1; fi; if [ -e \"$path\" ] && [ ! -d \"$path\" ]; then printf 'velnor_cache_not_directory:%s\\n' \"$relative\" >&2; exit 1; fi; done;",
    ];
    if has_checkout {
        commands.extend([
            "if [ -L \"$workspace/.git\" ] || [ ! -d \"$workspace/.git\" ]; then printf 'velnor_git_directory_invalid\\n' >&2; exit 1; fi;",
            "if [ -L \"$workspace/.git/info\" ] || [ ! -d \"$workspace/.git/info\" ]; then printf 'velnor_git_info_invalid\\n' >&2; exit 1; fi;",
            "exclude=\"$workspace/.git/info/exclude\"; if [ -L \"$exclude\" ] || { [ -e \"$exclude\" ] && [ ! -f \"$exclude\" ]; }; then printf 'velnor_git_exclude_invalid\\n' >&2; exit 1; fi;",
            "touch \"$exclude\"; rule='/.velnor/cache/'; if ! grep -Fqx \"$rule\" \"$exclude\"; then printf '\\n%s\\n' \"$rule\" >> \"$exclude\"; fi;",
            "git -C \"$workspace\" check-ignore -q .velnor/cache/seed",
        ]);
    } else {
        commands.push(
            "if [ -e \"$workspace/.git\" ] || [ -L \"$workspace/.git\" ]; then printf 'velnor_git_directory_unexpected_without_checkout\\n' >&2; exit 1; fi;",
        );
    }
    let script = commands.join(" ");
    crate::steps::shell_step(
        WORKSPACE_CACHE_GUARD_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        BTreeMap::new(),
    )
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}
