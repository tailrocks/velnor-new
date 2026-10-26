// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(crate) struct PlanCache {
    pub(crate) restore: Vec<Step>,
    pub(crate) save: Vec<Step>,
}

pub(crate) fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
) -> Result<PlanCache, OrchestratorError> {
    let mut restore = Vec::new();
    restore.extend(crate::cache_keys::cache_keys_step(
        catalog,
        fetch_roots,
        tofu_roots,
    )?);
    let mut save = Vec::new();
    if !fetch_roots.is_empty() {
        let (sources_restore, sources_save) = crate::source_cache::plan_sources_steps()?;
        restore.push(sources_restore);
        save.push(sources_save);
    }
    Ok(PlanCache { restore, save })
}

pub(crate) fn mark_writer_gate(source_steps: &mut [Step]) {
    if let Some(last) = source_steps.last_mut() {
        last.id = Some(StepId::SourcesFetch);
    }
}
