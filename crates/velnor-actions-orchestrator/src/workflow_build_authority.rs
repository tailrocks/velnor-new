// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, Job, OrchestratorError, PLAN_JOB_ID, PreparedWorkflow, Step, ToolCatalog,
    WorkflowBuildInput, WorkflowPolicy,
};

pub(super) fn attach_state_roots(
    input: &WorkflowBuildInput<'_>,
    prepared: &PreparedWorkflow,
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), OrchestratorError> {
    let state_roots = crate::state_roots::plans(input.discovery, jobs, &prepared.state_recipes)?;
    let seal = crate::state_roots::seal_plan(jobs)?;
    if state_roots.is_empty() && seal.roots.is_empty() {
        return Ok(());
    }
    let tofu_roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
    let plan = jobs
        .get_mut(PLAN_JOB_ID)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "plan_job_missing".to_owned(),
        })?;
    velnor_actions_workflow_renderer::native_job_builders::plan_state_roots(
        plan,
        &prepared.helper_catalog,
        input.fetch_roots,
        &tofu_roots,
        &state_roots,
        &seal,
    )
    .map_err(crate::workflow_jobs::map_native_job_builder_error)?;
    plan.permissions
        .get_or_insert_with(crate::workflow_jobs::read_actions_permissions);
    Ok(())
}

pub(super) fn required_controller(
    input: &WorkflowBuildInput<'_>,
    prepared: &mut PreparedWorkflow,
) -> Result<(ToolCatalog, Option<Step>), OrchestratorError> {
    let selected = if prepared.policy == WorkflowPolicy::VelnorRepositoryV1
        && prepared.acquire.as_ref().is_some_and(|step| {
            step.role == Some(velnor_actions_contract::StepRole::AcquireProtectedPreview)
        }) {
        (
            helper_catalog(input, &ToolCatalog::pinned())?,
            prepared.acquire.take(),
        )
    } else if prepared.policy == WorkflowPolicy::VelnorRepositoryV1 {
        let target = velnor_actions_contract::ReleaseTarget::for_runner_label(input.label)
            .ok_or_else(|| OrchestratorError::Contract {
                problem: format!("unsupported_target_for_runner:{}", input.label),
            })?;
        (
            velnor_actions_mise::catalog::lock::catalog_for_published_verdict(&ToolCatalog::pinned()),
            Some(velnor_actions_workflow_renderer::release_helpers::published_verdict_acquire_for_target(target)
                .map_err(crate::local_release_manifest::map_release_helper_error)?),
        )
    } else {
        (
            helper_catalog(input, &ToolCatalog::pinned())?,
            prepared.acquire.take(),
        )
    };
    Ok(selected)
}

pub(super) fn helper_catalog(
    input: &WorkflowBuildInput<'_>,
    catalog: &ToolCatalog,
) -> Result<ToolCatalog, OrchestratorError> {
    if input.config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(catalog.clone());
    }
    let lock = input
        .published_generator_lock
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "required_verdict_requires_published_generator_lock".to_owned(),
        })?;
    let map_err =
        |error: velnor_actions_mise::catalog::lock::LockError| OrchestratorError::Contract {
            problem: error.to_string(),
        };
    match input.approved_release {
        Some(approved) => velnor_actions_mise::catalog::lock::catalog_for_approved_generator(
            lock, catalog, &approved,
        )
        .map_err(map_err),
        None => Err(OrchestratorError::Contract {
            problem: "controller_approvals:missing_approval".to_owned(),
        }),
    }
}
