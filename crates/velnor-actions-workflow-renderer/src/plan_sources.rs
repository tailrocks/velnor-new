// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId};
use velnor_actions_mise::ToolCatalog;

use crate::RenderError;

pub mod cache_keys;
pub mod cargo_fetch;
pub mod source_cache;

#[derive(Debug)]
pub enum PlanSourcesError {
    Contract(velnor_actions_contract::ContractError),
    ShellRender(RenderError),
    CacheSourcesPolicy(String),
    CacheActionRef(String),
    Render(RenderError),
}

#[derive(Debug, Clone)]
pub struct PlanCache {
    pub restore: Vec<Step>,
    pub save: Vec<Step>,
}

pub fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
) -> Result<PlanCache, PlanSourcesError> {
    let mut restore = Vec::new();
    restore.extend(cache_keys::cache_keys_step(
        catalog,
        fetch_roots,
        tofu_roots,
    )?);
    let mut save = Vec::new();
    if !fetch_roots.is_empty() {
        let (sources_restore, sources_save) = source_cache::plan_sources_steps()?;
        restore.push(sources_restore);
        save.push(sources_save);
    }
    Ok(PlanCache { restore, save })
}

pub fn mark_writer_gate(source_steps: &mut [Step]) {
    if let Some(last) = source_steps.last_mut() {
        last.id = Some(StepId::SourcesFetch);
    }
}
