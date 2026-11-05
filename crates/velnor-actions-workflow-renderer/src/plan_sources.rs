// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::RenderError;

pub mod cache_keys;
pub mod cargo_fetch;
pub mod source_cache;
pub mod state_select;

#[derive(Debug)]
pub enum PlanSourcesError {
    Contract(velnor_actions_contract::ContractError),
    ShellRender(RenderError),
    CacheSourcesPolicy(String),
    CacheActionRef(String),
    Render(RenderError),
}

pub fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
    state_roots: &[cache_keys::StateUnitPlan],
) -> Result<Vec<Step>, PlanSourcesError> {
    let mut restore = Vec::new();
    restore.extend(cache_keys::cache_keys_step(
        catalog,
        fetch_roots,
        tofu_roots,
        state_roots,
    )?);
    if !fetch_roots.is_empty() {
        restore.push(source_cache::plan_sources_restore()?);
    }
    Ok(restore)
}
