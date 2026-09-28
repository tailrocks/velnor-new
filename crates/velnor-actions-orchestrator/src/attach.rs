//! Render-time job-IR attach: lock acquire plus pre-seed build-once.
//!
//! Both attach points run in `render_staged_tree` after `build_workflow`:
//! lock-backed digest staging when the bootstrap lock exists, explicit
//! pre-seed build-once steps (trust-on-review) when it does not. Consumer
//! generation never calls either.

use velnor_actions_contract::{GeneratorLock, WorkflowIr, target_for_runner_label};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID, TASK_JOB_ID};
use velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX;
use velnor_actions_workflow_renderer::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_stage_step, preseed_upload_step, preseed_verify_step,
};

use crate::OrchestratorError;
use crate::pins::lock_acquire_step;
use crate::vectors::{candidate_build_argv, mbx_probe_argv};
use crate::workflow::WorkflowPlan;

/// Attach lock-backed Acquire steps to the plan and final jobs.
///
/// Reads the runner-target record from an already-verified lock, stages the
/// digest-verified binary under `$RUNNER_TEMP`, and inserts the step between
/// checkout and plan plus ahead of the report merge. Consumer generation
/// never calls this: it embeds the release manifest instead, and never
/// reads the lock.
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
    plan.steps.insert(1, plan_step);
    let Some(final_gate) = ir.jobs.get_mut(FINAL_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "final_job_missing".to_owned(),
        });
    };
    final_gate.steps.insert(0, final_step);
    Ok(())
}

/// Attach explicit pre-seed build-once steps (Velnor policy, no lock).
///
/// The plan job builds the helper once from the checked-out source with
/// the fixed §4 vector, verifies the MBX compile output plus its pinned
/// route, records the source commit in a manifest, uploads the
/// exactly-named artifact, and stages its local build; task (when
/// present) and final jobs download that artifact and stage it instead
/// of rebuilding. Sets the render context's pre-seed mode so the strict
/// gates accept fixed pre-seed staging. Consumer generation never calls
/// this.
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
    let plan_steps = vec![
        preseed_build_step(build.clone())?,
        preseed_verify_step(&probe, catalog.version(PinnedTool::MrBoxington))?,
        preseed_manifest_step(&build, target)?,
        preseed_upload_step()?,
        preseed_stage_step(PreseedStageSource::LocalBuild, &staged)?,
    ];
    let Some(plan) = workflow.ir.jobs.get_mut(PLAN_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "plan_job_missing".to_owned(),
        });
    };
    plan.steps.splice(1..1, plan_steps);
    let Some(final_gate) = workflow.ir.jobs.get_mut(FINAL_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "final_job_missing".to_owned(),
        });
    };
    final_gate.steps.splice(
        0..0,
        [
            preseed_download_step()?,
            preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged)?,
        ],
    );
    if let Some(task) = workflow.ir.jobs.get_mut(TASK_JOB_ID) {
        task.steps.splice(
            1..1,
            [
                preseed_download_step()?,
                preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged)?,
            ],
        );
    }
    workflow.context.preseed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{CHECKOUT_USES, REQUEST_DIR};
    use crate::workflow_jobs::{final_job, plan_job, task_job};
    use std::collections::BTreeMap;
    use velnor_actions_contract::{Concurrency, Permissions, Trigger};
    use velnor_actions_workflow_renderer::render::{RenderContext, WORKFLOW_PATH};

    /// Bare IR shell shared by the attach fixtures.
    fn bare_ir(jobs: BTreeMap<String, velnor_actions_contract::Job>) -> WorkflowIr {
        WorkflowIr {
            name: "CI".to_owned(),
            triggers: Trigger {
                pull_request_types: Vec::new(),
                push_branches: Vec::new(),
                merge_group: false,
            },
            permissions: Permissions {
                contents: "read".to_owned(),
                actions: "read".to_owned(),
            },
            concurrency: Concurrency {
                group: "g".to_owned(),
                cancel_in_progress: "c".to_owned(),
            },
            jobs,
        }
    }

    #[test]
    fn lock_acquire_inserts_digest_verified_stage() {
        use velnor_actions_contract::{GeneratorBinary, LockedGenerator, MiseBootstrap};
        let lock = GeneratorLock {
            schema: 1,
            generator: LockedGenerator {
                binary: "velnor-actions".to_owned(),
                version: "0.1.0".to_owned(),
                binaries: vec![GeneratorBinary {
                    target: "x86_64-unknown-linux-gnu".to_owned(),
                    artifact: "https://example.invalid/r".to_owned(),
                    sha256: "a".repeat(64),
                }],
            },
            mise_bootstrap: MiseBootstrap {
                version: "2026.9.16".to_owned(),
                artifact: "https://example.invalid/m".to_owned(),
                sha256: "b".repeat(64),
            },
            actions: Vec::new(),
        };
        let mut ir = bare_ir(BTreeMap::from([
            ("velnor-plan".to_owned(), plan_job("ubuntu-26.04", None)),
            (
                "velnor-final".to_owned(),
                final_job("ubuntu-26.04", false, None),
            ),
        ]));
        assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").is_ok());
        let names: Vec<&str> = ir.jobs["velnor-plan"]
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Checkout", "Acquire Velnor", "Plan"]);
        let names: Vec<&str> = ir.jobs["velnor-final"]
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Acquire Velnor", "Merge reports"]);
        assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04-arm", "0.1.0").is_err());
    }

    #[test]
    fn preseed_attach_builds_once_and_sets_mode() {
        use velnor_actions_actionlint::ActionlintConfigInput;
        use velnor_actions_workflow_renderer::{
            PRESEED_BUILD_NAME, PRESEED_DOWNLOAD_NAME, PRESEED_STAGE_NAME,
        };
        let mut plan = WorkflowPlan {
            ir: bare_ir(BTreeMap::from([
                ("velnor-plan".to_owned(), plan_job("ubuntu-26.04", None)),
                ("velnor-task".to_owned(), task_job("ubuntu-26.04", 2)),
                (
                    "velnor-final".to_owned(),
                    final_job("ubuntu-26.04", true, None),
                ),
            ])),
            support: None,
            context: RenderContext {
                generator_version: "0.1.0".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                staged_binary: format!("{STAGED_BINARY_PREFIX}0.1.0"),
                request_dir: REQUEST_DIR.to_owned(),
                checkout_uses: CHECKOUT_USES.to_owned(),
                policy_commands: Vec::new(),
                candidate: None,
                preseed: false,
            },
            actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
        };
        assert!(attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").is_ok());
        assert!(plan.context.preseed);
        let names: Vec<&str> = plan.ir.jobs["velnor-plan"]
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Checkout",
                PRESEED_BUILD_NAME,
                "Verify MBX compile (pre-seed trust-on-review)",
                "Write helper manifest (pre-seed trust-on-review)",
                "Upload helper (pre-seed trust-on-review)",
                PRESEED_STAGE_NAME,
                "Plan",
            ]
        );
        for id in ["velnor-task", "velnor-final"] {
            let names: Vec<&str> = plan.ir.jobs[id]
                .steps
                .iter()
                .map(|s| s.name.as_str())
                .collect();
            assert!(
                names.contains(&PRESEED_DOWNLOAD_NAME) && names.contains(&PRESEED_STAGE_NAME),
                "{id}: {names:?}"
            );
            assert!(
                !names.contains(&PRESEED_BUILD_NAME),
                "{id} must not rebuild: {names:?}"
            );
        }
        assert!(attach_preseed(&mut plan, "ubuntu-26.04-arm", "0.1.0").is_err());
    }
}
