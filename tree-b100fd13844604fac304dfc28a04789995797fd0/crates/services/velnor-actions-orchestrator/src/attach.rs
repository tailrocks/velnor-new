//! Render-time job-IR attach: lock acquire plus pre-seed build-once.
//!
//! Both attach points run in `render_staged_tree` after `build_workflow`:
//! lock-backed digest staging when the bootstrap lock exists, explicit
//! pre-seed build-once steps (trust-on-review) when it does not. Consumer
//! generation never calls either.

use std::collections::BTreeMap;

use velnor_actions_contract::is_crate_job_id;
use velnor_actions_contract_release::{GeneratorLock, ReleaseTarget};
use velnor_actions_contract_workflow::{Step, StepRole, WorkflowIr};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID};
use velnor_actions_workflow_renderer::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};
use velnor_actions_workflow_steps::steps::STAGED_BINARY_PREFIX;

use crate::OrchestratorError;
use crate::pins::{lock_acquire_for_runner, lock_acquire_step};
use crate::vectors::{candidate_build_argv, mbx_probe_argv};
use crate::workflow::WorkflowPlan;

/// Attach lock-backed Acquire steps to plan, final, publish, and crate jobs.
///
/// Reads the runner-target record from an already-verified lock, stages the
/// digest-verified binary under `$RUNNER_TEMP`, and inserts the step between
/// checkout and plan plus ahead of the report merge and the baseline
/// publish; every crate job stages right after checkout because its
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
    // The publish job invokes the staged helper too; absent jobs stay
    // untouched like zero-crate plans.
    if let Some(publish) = ir.jobs.get_mut(PUBLISH_JOB_ID) {
        publish
            .steps
            .insert(0, lock_acquire_step(lock, label, &staged)?);
    }
    for (id, job) in &mut ir.jobs {
        if let Some(runner) = &job.check_runner {
            let step = lock_acquire_for_runner(lock, runner, &staged)?;
            if !job.steps.iter().any(|step| step.name == "Acquire Velnor") {
                job.steps.insert(1.min(job.steps.len()), step);
            }
        } else if is_crate_job_id(id) {
            let step = lock_acquire_step(lock, label, &staged)?;
            job.steps.insert(1, step);
        }
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
    fetch_roots: &[String],
) -> Result<(), OrchestratorError> {
    let catalog = ToolCatalog::pinned();
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
    let build = candidate_build_argv(&catalog)?;
    let probe = mbx_probe_argv(&catalog)?;
    let staged = format!("{STAGED_BINARY_PREFIX}{version}");
    let homes = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)?;
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
    insert_plan_mbx_restore(&catalog, label, fetch_roots, &mut plan.steps)?;
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
    if let Some(publish) = workflow.ir.jobs.get_mut(PUBLISH_JOB_ID) {
        publish
            .steps
            .splice(0..0, preseed_consumers(target, &staged)?);
    }
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
fn insert_plan_mbx_restore(
    catalog: &ToolCatalog,
    label: &str,
    fetch_roots: &[String],
    steps: &mut Vec<Step>,
) -> Result<(), OrchestratorError> {
    if steps.iter().any(is_mbx_action) {
        return Err(OrchestratorError::Contract {
            problem: "preseed_mbx_action_duplicated".to_owned(),
        });
    }
    replace_plan_registry_cache(catalog, label, fetch_roots, steps)?;
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

/// Replace Swatinem's Cargo-only plan cache with the shared registry snapshot.
///
/// MBX object storage and the source subset remain separate owners. The
/// registry-only fallback cannot coexist with the MBX action in one job,
/// so lockful pre-seed plans use the same source cache as MBX workflows.
fn replace_plan_registry_cache(
    catalog: &ToolCatalog,
    label: &str,
    fetch_roots: &[String],
    steps: &mut Vec<Step>,
) -> Result<(), OrchestratorError> {
    let registry: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::CargoRegistryRestore))
        .map(|(index, _)| index)
        .collect();
    if registry.is_empty() {
        return Ok(());
    }
    if registry.len() != 1 || fetch_roots.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "preseed_registry_cache_without_locked_sources".to_owned(),
        });
    }
    let target = ReleaseTarget::for_runner_label(label)
        .map(ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    let key = crate::source_cache::sources_cache_key(
        target,
        catalog.version(PinnedTool::Rust),
        fetch_roots,
    )?;
    let prefix = crate::source_cache::sources_restore_prefix(&key);
    steps[registry[0]] = crate::source_cache::sources_restore_step(&key, &[prefix])?;
    let save = crate::source_cache::sources_save_step(&key)?;
    let after_source_collection = steps
        .iter()
        .rposition(|step| step.role == Some(StepRole::CargoSourcesFetch))
        .map(|index| index + 1)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "preseed_registry_cache_missing_source_step".to_owned(),
        })?;
    steps.insert(after_source_collection, save);
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
    step.role == Some(StepRole::MbxCache)
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
            step.role == Some(StepRole::CargoSourcesFetch)
                || is_plan_restore(step)
                || step.role == Some(StepRole::MbxVersionCheck)
        })
        .map(|(index, _)| index)
        .max();
    last_required.map_or_else(|| after_prepare(steps), |index| index + 1)
}

/// True for plan-job restore steps (shared, registry, MBX objects).
fn is_plan_restore(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::CargoSourcesRestore | StepRole::CargoRegistryRestore | StepRole::MbxCache)
    )
}

#[cfg(test)]
mod tests;
