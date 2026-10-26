// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepRole};

#[path = "cache_steps_tools_restore.rs"]
mod restore_action;

use crate::RenderError;

pub const TOOLS_RESTORE_ACTION_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub const TOOLS_RESTORE_USES: &str =
    velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_RESTORE_USES;
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
pub(crate) const TOOLS_SEED_ADMITTED_INPUT: &str = "seed-admitted";
pub(crate) const TOOLS_MISE_SHA256_INPUT: &str = "mise-sha256";
pub(crate) const TOOLS_SEED_ADMITTED_EXPRESSION: &str = "${{ steps.v2.outputs.seed_admitted }}";
pub const TOOLS_CACHE_PATH: &str = "~/.local/share/mise";
pub const TOOLS_CACHE_PATHS: [&str; 5] = [
    TOOLS_CACHE_PATH,
    "${{ runner.temp }}/velnor/rustup",
    "${{ runner.temp }}/velnor/cargo/.crates.toml",
    "${{ runner.temp }}/velnor/cargo/.crates2.json",
    "${{ runner.temp }}/velnor/cargo/bin",
];
pub(super) fn tools_cache_path_ok(path: &str) -> bool {
    TOOLS_CACHE_PATHS.contains(&path)
}

pub(crate) fn validate_restore_call(step: &Step) -> Result<&str, RenderError> {
    restore_action::validate_call(step)
}

pub(super) fn restore_step(
    key: &str,
    mise_sha256: &str,
    condition: Option<String>,
) -> Result<Step, RenderError> {
    if !crate::cache_p08::is_v2_cache_key_expression(key) {
        return Err(RenderError::InvalidWorkflow(
            "bad_tools_cache_restore_key".to_owned(),
        ));
    }
    if !restore_action::is_mise_sha256(mise_sha256) {
        return Err(RenderError::InvalidWorkflow(
            "bad_tools_cache_restore_mise_sha256".to_owned(),
        ));
    }
    let mut step = crate::steps::action_step(
        TOOLS_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        std::collections::BTreeMap::from([
            ("key".to_owned(), key.to_owned()),
            (TOOLS_MISE_SHA256_INPUT.to_owned(), mise_sha256.to_owned()),
            (
                TOOLS_SEED_ADMITTED_INPUT.to_owned(),
                TOOLS_SEED_ADMITTED_EXPRESSION.to_owned(),
            ),
        ]),
    )?;
    step.condition = condition;
    step.id = Some(velnor_actions_contract::StepId::ToolsRestore);
    step.role = Some(StepRole::ToolsCacheRestore);
    Ok(step)
}

pub(super) fn save_step(key: &str, condition: Option<String>) -> Result<Step, RenderError> {
    let mut step = super::cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "tools",
        key,
        &[],
        &TOOLS_CACHE_PATHS.map(str::to_owned),
    )?;
    crate::steps::scan_for_private_subcommands(TOOLS_SAVE_NAME)?;
    TOOLS_SAVE_NAME.clone_into(&mut step.name);
    step.condition = condition;
    step.role = Some(StepRole::ToolsCacheSave);
    Ok(step)
}

pub(crate) fn restore_action_file(
    version: &str,
    tool_pins: &[String],
) -> Result<crate::tree::RenderedFile, RenderError> {
    restore_action::action_file(version, tool_pins)
}
