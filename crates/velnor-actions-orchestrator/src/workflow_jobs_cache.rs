// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId, StepRole};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(crate) struct PlanCache {
    pub(crate) restore: Vec<Step>,
    pub(crate) save: Vec<Step>,
}

fn is_plan_cache_save(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::ToolsCacheSave
            | StepRole::CargoSourcesSave
            | StepRole::MbxBundleSave
            | StepRole::FamilySave
            | StepRole::TofuProvidersSave
    )
}

fn is_plan_cache_restore(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::ToolsCacheRestore
            | StepRole::CargoSourcesRestore
            | StepRole::MbxBundleRestore
            | StepRole::FamilyRestore
            | StepRole::TofuProvidersRestore
    )
}

fn check_plan_cache_direction(restore: &[Step], save: &[Step]) -> Result<(), OrchestratorError> {
    if restore
        .iter()
        .any(|step| step.role.is_some_and(is_plan_cache_save))
    {
        return Err(OrchestratorError::Contract {
            problem: "plan_cache_save_in_restore".to_owned(),
        });
    }
    if save
        .iter()
        .any(|step| step.role.is_some_and(is_plan_cache_restore))
    {
        return Err(OrchestratorError::Contract {
            problem: "plan_cache_restore_in_save".to_owned(),
        });
    }
    Ok(())
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
    check_plan_cache_direction(&restore, &save)?;
    Ok(PlanCache { restore, save })
}

pub(crate) fn mark_writer_gate(source_steps: &mut [Step]) {
    if let Some(last) = source_steps.last_mut() {
        last.id = Some(StepId::SourcesFetch);
    }
}
