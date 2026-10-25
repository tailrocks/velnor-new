use std::collections::BTreeMap;

use velnor_actions_contract::{
    GeneratorLock, ReleaseTarget, Step, StepRole, WorkflowIr, is_crate_job_id,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::steps::{MbxTransport, STAGED_BINARY_PREFIX};
use velnor_actions_workflow_renderer::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};

use crate::OrchestratorError;
use crate::pins::{lock_acquire_for_runner, lock_acquire_step};
use crate::vectors::{candidate_build_argv, mbx_probe_argv};
use crate::workflow::WorkflowPlan;

pub(crate) fn attach_lock_acquire(
    ir: &mut WorkflowIr,
    lock: &GeneratorLock,
    label: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    let staged = format!("{STAGED_BINARY_PREFIX}{version}");
    let plan_step = lock_acquire_step(lock, label, &staged)?;
    let final_step = lock_acquire_step(lock, label, &staged)?;
    let Some(plan) = ir.jobs.get_mut(PLAN_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "plan_job_missing".to_owned(),
        });
    };
    let at = after_prepare(&plan.steps);
    plan.steps.insert(at, plan_step);
    let Some(final_gate) = ir.jobs.get_mut(FINAL_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "final_job_missing".to_owned(),
        });
    };
    final_gate.steps.insert(0, final_step);
    for (id, job) in &mut ir.jobs {
        if let Some(runner) = &job.check_runner {
            let step = lock_acquire_for_runner(lock, runner, &staged)?;
            if !job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::AcquireVelnor))
            {
                job.steps.insert(1.min(job.steps.len()), step);
            }
        } else if is_crate_job_id(id) {
            let step = lock_acquire_step(lock, label, &staged)?;
            job.steps.insert(1, step);
        }
    }
    Ok(())
}

pub(crate) fn attach_preseed(
    workflow: &mut WorkflowPlan,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    let target = ReleaseTarget::for_runner_label(label)
        .map(ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    for (id, job) in &workflow.ir.jobs {
        if let Some(runner) = &job.check_runner
            && runner.platform.target() != target
        {
            return Err(OrchestratorError::Contract {
                problem: format!("mixed_platform_preseed_requires_release_lock:{id}"),
            });
        }
    }
    let build = candidate_build_argv(catalog)?;
    let probe = mbx_probe_argv(catalog)?;
    let staged = format!("{STAGED_BINARY_PREFIX}{version}");
    let homes = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let plan_steps = vec![
        preseed_build_step(&build, &homes)?,
        preseed_verify_step(&probe, catalog.version(PinnedTool::MrBoxington), &homes)?,
        preseed_manifest_step(&build, target)?,
        preseed_upload_step()?,
        preseed_stage_step(PreseedStageSource::LocalBuild, &staged)?,
    ];
    let Some(plan) = workflow.ir.jobs.get_mut(PLAN_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "plan_job_missing".to_owned(),
        });
    };
    insert_plan_mbx_restore(catalog, &mut plan.steps)?;
    let at = preseed_anchor(&plan.steps);
    plan.steps.splice(at..at, plan_steps);
    let Some(final_gate) = workflow.ir.jobs.get_mut(FINAL_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "final_job_missing".to_owned(),
        });
    };
    final_gate
        .steps
        .splice(0..0, preseed_consumers(target, &staged)?);
    for (id, job) in &mut workflow.ir.jobs {
        if is_crate_job_id(id) || job.check_runner.is_some() {
            let at = 1.min(job.steps.len());
            job.steps
                .splice(at..at, preseed_consumers(target, &staged)?);
        }
    }
    workflow.context.preseed = true;
    Ok(())
}

fn preseed_consumers(target: &str, staged: &str) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![
        preseed_download_step()?,
        preseed_manifest_verify_step(target)?,
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, staged)?,
    ])
}

fn after_prepare(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        .map_or(1, |index| index + 1)
}

fn insert_plan_mbx_restore(
    catalog: &ToolCatalog,
    steps: &mut Vec<Step>,
) -> Result<(), OrchestratorError> {
    if steps.iter().any(is_mbx_action) {
        return Err(OrchestratorError::Contract {
            problem: "preseed_mbx_action_duplicated".to_owned(),
        });
    }
    let restore_at = steps
        .iter()
        .position(|step| step.role == Some(StepRole::CargoSourcesRestore))
        .map_or_else(|| after_rust_setup(steps), |index| index + 1);
    for (offset, step) in
        crate::mbx_preflight::steps_for_catalog(catalog, MbxTransport::InstallOnly, None)?
            .into_iter()
            .enumerate()
    {
        steps.insert(restore_at + offset, step);
    }
    Ok(())
}

fn after_rust_setup(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| step.name == crate::workflow::VERIFY_RUST_MANIFEST_STEP)
        .or_else(|| {
            steps
                .iter()
                .position(|step| step.role == Some(StepRole::PrepareRustComponents))
        })
        .or_else(|| {
            steps
                .iter()
                .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        })
        .map_or(1, |index| index + 1)
}

fn is_mbx_action(step: &Step) -> bool {
    step.role == Some(StepRole::MbxCache)
}

fn preseed_anchor(steps: &[Step]) -> usize {
    let last_required = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(
                step.role,
                Some(
                    StepRole::CargoSourcesFetch
                        | StepRole::CargoSourcesRestore
                        | StepRole::MbxCache
                        | StepRole::MbxVersionCheck
                )
            )
        })
        .map(|(index, _)| index)
        .max();
    last_required.map_or_else(|| after_prepare(steps), |index| index + 1)
}
