//! Render-time job-IR attach: lock acquire plus pre-seed build-once.
//!
//! Both attach points run in `render_staged_tree` after `build_workflow`:
//! lock-backed digest staging when the bootstrap lock exists, explicit
//! pre-seed build-once steps (trust-on-review) when it does not. Consumer
//! generation never calls either.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    GeneratorLock, Step, WorkflowIr, is_crate_job_id, target_for_runner_label,
};
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::cache_p08::{RESTORE_SOURCES_NAME, RUST_CACHE_NAME};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID};
use velnor_actions_workflow_renderer::steps::{MBX_RESTORE_NAME, STAGED_BINARY_PREFIX};
use velnor_actions_workflow_renderer::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};

use crate::OrchestratorError;
use crate::pins::lock_acquire_step;
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
        if !is_crate_job_id(id) {
            continue;
        }
        let step = lock_acquire_step(lock, label, &staged)?;
        job.steps.insert(1, step);
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
) -> Result<(), OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let target = target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("unsupported_target_for_runner:{label}"),
    })?;
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
    insert_plan_mbx_restore(&catalog, &mut plan.steps)?;
    let at = preseed_anchor(&plan.steps);
    plan.steps.splice(at..at, plan_steps);
    let Some(final_gate) = workflow.ir.jobs.get_mut(FINAL_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "final_job_missing".to_owned(),
        });
    };
    final_gate.steps.splice(
        0..0,
        [
            preseed_download_step()?,
            preseed_manifest_verify_step(target)?,
            preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged)?,
        ],
    );
    // The publish job invokes the staged helper too; without the
    // triple its internal steps fail the staged gate. Absent jobs
    // stay untouched like zero-crate plans.
    if let Some(publish) = workflow.ir.jobs.get_mut(PUBLISH_JOB_ID) {
        publish.steps.splice(
            0..0,
            [
                preseed_download_step()?,
                preseed_manifest_verify_step(target)?,
                preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged)?,
            ],
        );
    }
    for (id, job) in &mut workflow.ir.jobs {
        if !is_crate_job_id(id) {
            continue;
        }
        job.steps.splice(
            1..1,
            [
                preseed_download_step()?,
                preseed_manifest_verify_step(target)?,
                preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged)?,
            ],
        );
    }
    workflow.context.preseed = true;
    Ok(())
}

/// Insert index for plan-job provisioning: after `Prepare pinned tools`.
///
/// Contract order is Checkout < Setup Mise < Prepare < Acquire; the
/// fallback covers hand-built fixtures without the install step.
fn after_prepare(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .map_or(1, |index| index + 1)
}

/// Insert the plan-job MBX objects restore ahead of fetch (pre-seed only).
///
/// The pre-seed build compiles through MBX on every repo, so the plan
/// job warms the object store exactly like an MBX crate job: right after
/// the shared-sources restore, ahead of fetch. Plans without a shared
/// restore (cargo-only rust-cache writers, lockless) stay untouched: an
/// MBX step beside a rust-cache writer would trip the P08 one-owner
/// gate, and there is nothing to warm without sources.
fn insert_plan_mbx_restore(
    catalog: &ToolCatalog,
    steps: &mut Vec<Step>,
) -> Result<(), OrchestratorError> {
    let Some(restore_at) = steps
        .iter()
        .position(|step| step.name == RESTORE_SOURCES_NAME)
    else {
        return Ok(());
    };
    for (offset, step) in crate::mbx_preflight::steps_for_catalog(catalog)?
        .into_iter()
        .enumerate()
    {
        steps.insert(restore_at + 1 + offset, step);
    }
    Ok(())
}

/// Insert index for the plan-job pre-seed build block.
///
/// The build compiles code, so it runs after the source-probing steps
/// that guarantee sources present (which is after every restore);
/// without them it anchors after the last restore, and the fallback
/// covers lockless plans and hand-built fixtures without cache steps.
fn preseed_anchor(steps: &[Step]) -> usize {
    if let Some(last) = steps.iter().rposition(|step| {
        step.name
            .starts_with(crate::source_prep::FETCH_SOURCES_STEP)
    }) {
        return last + 1;
    }
    if let Some(last) = steps.iter().rposition(|step| is_plan_restore(&step.name)) {
        return last + 1;
    }
    after_prepare(steps)
}

/// True for plan-job restore steps (shared, registry, MBX objects).
fn is_plan_restore(name: &str) -> bool {
    name == RESTORE_SOURCES_NAME || name == RUST_CACHE_NAME || name == MBX_RESTORE_NAME
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod attach_tests;
