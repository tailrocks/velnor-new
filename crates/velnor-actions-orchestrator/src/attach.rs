//! Render-time job-IR attach: lock acquire plus pre-seed build-once.
//!
//! Both attach points run in `render_staged_tree` after `build_workflow`:
//! lock-backed digest staging when the bootstrap lock exists, explicit
//! pre-seed build-once steps (trust-on-review) when it does not. Consumer
//! generation never calls either.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    GeneratorLock, ReleaseTarget, Step, StepRole, WorkflowIr, is_crate_job_id,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX;
use velnor_actions_workflow_renderer::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};

use crate::OrchestratorError;
use crate::pins::{lock_acquire_for_runner, lock_acquire_step};
use crate::vectors::{candidate_build_argv, mbx_probe_argv};
use crate::workflow::WorkflowPlan;

/// Attach lock-backed Acquire steps to plan, final, and crate jobs.
///
/// Reads the runner-target record from an already-verified lock, stages the
/// digest-verified binary under `$RUNNER_TEMP`, and inserts the step between
/// checkout and plan plus ahead of the report merge; every crate job stages right after checkout because its
/// obligation wrappers invoke the helper. Consumer generation never
/// calls this: it embeds the release manifest instead, and never reads
/// the lock.
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

/// The Cache seed jobs acquire the lock-backed helper like crate jobs.
/// # Errors
///
/// Returns lock errors for a runner target without a lock record.
pub(crate) fn attach_seed_lock_acquire(
    seed: &mut crate::cache_seed::SeedSpec,
    lock: &GeneratorLock,
    label: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    if !seed.is_empty() {
        let staged = format!("{STAGED_BINARY_PREFIX}{version}");
        seed.acquire = Some(lock_acquire_step(lock, label, &staged)?);
    }
    Ok(())
}

/// Attach explicit pre-seed build-once steps (Velnor policy, no lock).
///
/// The plan job restores the MBX object store (shared-snapshot plans),
/// builds the helper once from the checked-out source with the fixed §4
/// vector after the fetch steps that guarantee sources present, verifies
/// the MBX compile output plus its pinned route, records the source
/// commit in a manifest, uploads the exactly-named artifact, and stages
/// its local build; every crate job, the final job, and the publish
/// job download that artifact, verify the manifest (commit anchored
/// to this run's
/// `$GITHUB_SHA`, generator-rendered target, recomputed sha256), and
/// stage it instead of rebuilding. Build and verify run under the same
/// owned homes as the fetch steps. The gate binds staged bytes to this
/// run's checkout and across artifact transit; it does not review the
/// source (see the renderer's pre-seed docs for the trust boundary).
/// Sets the render context's
/// pre-seed mode so the strict gates accept fixed pre-seed staging.
/// Consumer generation never calls this.
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
    attach_seed_helper(workflow, &build, target)?;
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
    insert_plan_mbx_install(catalog, &mut plan.steps)?;
    let at = preseed_anchor(&plan.steps);
    plan.steps.splice(at..at, plan_steps);
    if !workflow.seed.is_empty() {
        // The helper dependency seed restores right before `Build helper`.
        let build_at = plan
            .steps
            .iter()
            .position(|step| step.role == Some(StepRole::PreseedBuild))
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "preseed_build_missing".to_owned(),
            })?;
        plan.steps.insert(build_at, helper_seed_restore()?);
    }
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

/// Pre-seed mode completes the seed spec and the Plan key step with the
/// helper dependency seed (`rsdeps-helper`, built in Plan with this argv).
fn attach_seed_helper(
    workflow: &mut WorkflowPlan,
    build: &[String],
    target: &str,
) -> Result<(), OrchestratorError> {
    if workflow.seed.is_empty() {
        return Ok(());
    }
    workflow.seed.keys.seed.helper_build = Some(build.to_vec());
    workflow.seed.plan_keys.seed.helper_build = Some(build.to_vec());
    workflow.seed.preseed_target = Some(target.to_owned());
    let key_step =
        crate::cache_keys::cache_keys_step(workflow.seed.catalog()?, &workflow.seed.plan_keys)?;
    let plan =
        workflow
            .ir
            .jobs
            .get_mut(PLAN_JOB_ID)
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "plan_job_missing".to_owned(),
            })?;
    let slot = plan
        .steps
        .iter_mut()
        .find(|step| step.id == Some(velnor_actions_contract::StepId::CacheKeys))
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "plan_cache_keys_missing".to_owned(),
        })?;
    if let Some(step) = key_step {
        *slot = step;
    }
    Ok(())
}

fn helper_seed_restore() -> Result<Step, OrchestratorError> {
    use velnor_actions_contract::workflow::cache_family::{
        CacheFamily, RSDEPS_HELPER_KEY_STEP_EXPR, RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR,
    };
    Ok(velnor_actions_workflow_renderer::steps::seed_cache_step(
        &crate::cache_seed::cache_uses(true)?,
        CacheFamily::RsdepsHelper,
        "",
        velnor_actions_workflow_renderer::steps::SeedCacheUse::Restore,
        RSDEPS_HELPER_KEY_STEP_EXPR,
        Some(RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR),
    )?)
}

/// Cache seed helper consumers: the triggering run's artifact, verified
/// against its head commit, then staged like every other consumer.
/// # Errors
pub(crate) fn seed_preseed_consumers(
    target: &str,
    staged: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    use velnor_actions_workflow_renderer::preseed_seed::{
        seed_preseed_download_step, seed_preseed_manifest_verify_step,
    };
    Ok(vec![
        seed_preseed_download_step()?,
        seed_preseed_manifest_verify_step(target)?,
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, staged)?,
    ])
}

/// Same-target consumers verify the artifact before staging its bytes.
fn preseed_consumers(target: &str, staged: &str) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![
        preseed_download_step()?,
        preseed_manifest_verify_step(target)?,
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, staged)?,
    ])
}

/// Insert index for plan-job provisioning: after `Prepare pinned tools`.
///
/// Contract order is Checkout < Setup Mise < Prepare < Acquire; the
/// fallback covers hand-built fixtures without the install step.
fn after_prepare(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        .map_or(1, |index| index + 1)
}

/// Insert the native MBX owner before the pre-seed build.
///
/// Lockful Cargo-only plans replace their registry-only fallback with the
/// canonical shared-sources cache, then restore MBX after that action.
/// Lockless plans have no source cache and install MBX after Rust setup.
fn insert_plan_mbx_install(
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
    for (offset, step) in crate::mbx_preflight::steps_for_catalog(catalog)?
        .into_iter()
        .enumerate()
    {
        steps.insert(restore_at + offset, step);
    }
    Ok(())
}

/// Insert after the pinned Rust setup and any separate component selection.
fn after_rust_setup(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| step.role == Some(StepRole::PrepareRustComponents))
        .or_else(|| {
            steps
                .iter()
                .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        })
        .map_or(1, |index| index + 1)
}

fn is_mbx_action(step: &Step) -> bool {
    step.role == Some(StepRole::MbxInstall)
}

/// Insert index for the plan-job pre-seed build block.
///
/// The build compiles code, so it runs after the source-probing steps
/// that guarantee sources present (which is after every restore);
/// without them it anchors after the last restore, and the fallback
/// covers lockless plans and hand-built fixtures without cache steps.
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
                        | StepRole::MbxInstall
                        | StepRole::MbxVersionCheck
                )
            )
        })
        .map(|(index, _)| index)
        .max();
    last_required.map_or_else(|| after_prepare(steps), |index| index + 1)
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod attach_tests;
