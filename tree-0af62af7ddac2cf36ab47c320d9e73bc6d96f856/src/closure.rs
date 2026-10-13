//! Mandated plan-job closure: provisioning gates plus freshness/publish.
//!
//! Covers the bootstrap state machine (bootstrap-and-release-contract
//! §§2-4): internal plan/merge steps must follow a digest-verified
//! `Acquire Velnor` step, and the plan job must check generated-tree
//! freshness plus publish its report (workflow-contract §3 steps 6, 8).
//! Provenance the contract does not allow fails closed with the
//! documented seed remediation instead of emitting a dead helper call.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError,
    closure_paths::{validate_helper_path, validate_output_dir},
    preseed,
    render::{PLAN_JOB_ID, RenderContext},
    steps,
};

/// Contract-fixed display name of the freshness step.
pub const CHECK_GENERATED_NAME: &str = "Check generated files";
/// Contract-fixed display name of the plan-report upload step.
pub const PUBLISH_PLAN_NAME: &str = "Publish plan";
/// Contract-fixed display name of the final plan-download step.
pub const DOWNLOAD_PLAN_NAME: &str = "Download plan";
/// Freshness preview root, verbatim from workflow-contract §3 step 6.
pub const FRESHNESS_OUTDIR: &str =
    "$RUNNER_TEMP/velnor-actions-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}";
/// Plan artifact name with the derived run key (workflow-contract §4).
pub const PLAN_ARTIFACT_NAME: &str = "velnor-plan-r${{ github.run_id }}-a${{ github.run_attempt }}";
/// Plan artifact source dir: the run-key directory under runner temp.
pub const PLAN_ARTIFACT_PATH: &str =
    "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ github.run_attempt }}";
/// Pre-seed remediation (bootstrap-and-release-contract §§4.1, 4.6).
pub const SEED_REMEDIATION: &str = "seed_required:publish the bootstrap release (manual pinned-Mise+MBX seed build), record it in .velnor/generator.lock, then regenerate";

/// Typed helper provenance for one plan/task/final job.
///
/// Release provenance stages the exact-version asset with SHA-256
/// verification; anything else is the documented manual seed, which
/// has no YAML emission and fails generation closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperProvenance {
    /// Release asset: immutable URL plus expected SHA-256.
    ReleaseAsset {
        /// Immutable `https://` asset URL for the runner target.
        url: String,
        /// Expected lowercase hex SHA-256 of the asset.
        sha256: String,
        /// Source commit (required, F3): manifest and lock paths both supply it.
        commit: String,
    },
    /// No provenance: the bootstrap release is not seeded yet.
    SeedRequired,
}

/// Acquire step from typed provenance; the seed path fails closed.
///
/// Release provenance builds the digest-verified staging step over
/// caller-supplied fixed argv (download plus a supported SHA-256 verifier);
/// the seed path emits nothing and reports the documented remediation.
/// # Errors
pub fn provision_acquire_step(
    provenance: &HelperProvenance,
    argv: Vec<String>,
) -> Result<Step, RenderError> {
    match provenance {
        HelperProvenance::ReleaseAsset {
            url,
            sha256,
            commit,
        } => {
            let env = std::collections::BTreeMap::from([
                (steps::ASSET_URL_ENV.to_owned(), url.clone()),
                (steps::ASSET_SHA_ENV.to_owned(), sha256.clone()),
                (steps::RELEASE_COMMIT_ENV.to_owned(), commit.clone()),
            ]);
            steps::acquire_velnor_step(argv, &env)
        }
        HelperProvenance::SeedRequired => {
            Err(RenderError::InvalidWorkflow(SEED_REMEDIATION.to_owned()))
        }
    }
}

/// Every internal step must follow a digest-verified Acquire step.
///
/// Internal steps invoke the staged helper; without a preceding
/// Acquire the path does not exist and CI fails with exit 127, so
/// generation fails closed here instead of emitting the dead call.
/// In pre-seed mode the fixed pre-seed stage step stages instead, and
/// only there: the stage name outside pre-seed mode fails closed.
/// # Errors
pub(crate) fn check_internal_staged(
    job_id: &str,
    job: &Job,
    preseed: bool,
) -> Result<(), RenderError> {
    let mut staged = false;
    for step in &job.steps {
        if is_acquire_step(step) {
            check_acquire_shape(job_id, step)?;
            staged = true;
        } else if step.name == steps::ACQUIRE_NAME {
            return Err(RenderError::InvalidWorkflow(format!(
                "acquire_malformed:{job_id}"
            )));
        } else if step.name == preseed::PRESEED_STAGE_NAME {
            if !preseed {
                return Err(RenderError::InvalidWorkflow(format!(
                    "preseed_stage_without_mode:{job_id}"
                )));
            }
            check_preseed_stage_shape(job_id, step)?;
            staged = true;
        } else if matches!(step.kind, StepKind::Internal { .. }) && !staged {
            return Err(RenderError::InvalidWorkflow(format!(
                "internal_without_acquire:{job_id}"
            )));
        }
    }
    Ok(())
}

/// Reject stage-named steps that do not copy a fixed origin to staging.
fn check_preseed_stage_shape(job_id: &str, step: &Step) -> Result<(), RenderError> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_stage_malformed:{job_id}"
        )));
    };
    let copies = run.iter().any(|arg| arg.contains("cp "))
        && run
            .iter()
            .any(|arg| arg.contains(steps::STAGED_BINARY_PREFIX))
        && run.iter().any(|arg| {
            arg.contains(preseed::PRESEED_BUILD_OUTPUT)
                || arg.contains(preseed::PRESEED_DOWNLOADED_BINARY)
        });
    if copies {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "preseed_stage_malformed:{job_id}"
        )))
    }
}

/// True for digest-verified Acquire steps (name plus asset env keys).
pub(crate) fn is_acquire_step(step: &Step) -> bool {
    step.name == steps::ACQUIRE_NAME
        && matches!(&step.kind, StepKind::Shell { env, .. } if env.contains_key(steps::ASSET_SHA_ENV) && env.contains_key(steps::ASSET_URL_ENV))
}

/// Reject Acquire-named steps whose staging shape is incomplete.
fn check_acquire_shape(job_id: &str, step: &Step) -> Result<(), RenderError> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "acquire_malformed:{job_id}"
        )));
    };
    let wired = run
        .iter()
        .any(|arg| arg.contains(steps::STAGED_BINARY_PREFIX))
        && env
            .get(steps::ASSET_SHA_ENV)
            .is_some_and(|sha| sha.len() == 64)
        && env
            .get(steps::ASSET_URL_ENV)
            .is_some_and(|url| url.starts_with("https://"));
    if wired {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "acquire_malformed:{job_id}"
        )))
    }
}

/// Fixed freshness step: regenerate into scratch, byte-compare `.github`.
///
/// Runs the helper's `generate --output-dir`, diffs preview against
/// committed `.github`; `env` is the caller-validated consumer env,
/// scrubbed here since local generation needs no ambient auth.
/// # Errors
pub fn freshness_step(
    binary: &str,
    output_dir: &str,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_helper_path(binary)?;
    validate_output_dir(output_dir)?;
    let script = format!(
        "{binary} generate --output-dir \"{output_dir}\" && diff -r --brief .github \"{output_dir}/.github\""
    );
    let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
    steps::shell_step(CHECK_GENERATED_NAME, argv, env.clone())
}

/// Fixed plan-report upload step (`velnor-plan-<run-key>`, fails loud).
///
/// Uploads the run-key directory (contract §4: `plan.json` plus
/// `matrix.json`); `if-no-files-found: error` fails closed when the
/// planner wrote nothing. `if: always()` is attached at render.
/// # Errors
pub fn publish_plan_step() -> Result<Step, RenderError> {
    steps::action_step(
        PUBLISH_PLAN_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PLAN_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PLAN_ARTIFACT_PATH.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}

/// Fixed plan-artifact download step (fails loud when absent).
///
/// Mirrors [`publish_plan_step`]: same name and directory, so the pair
/// agrees by construction. No `if:`: the job-level `always()` governs.
/// # Errors
pub fn download_plan_step() -> Result<Step, RenderError> {
    steps::action_step(
        DOWNLOAD_PLAN_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PLAN_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PLAN_ARTIFACT_PATH.to_owned()),
        ]),
    )
}

/// Insert `Download plan` before the merge write-request, once.
///
/// Without the download the final job has no plan to merge. Without a
/// final job there is nothing to close over. Re-running never dupes.
/// # Errors
pub(crate) fn insert_final_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let Some(final_job) = jobs.get_mut(crate::render::FINAL_JOB_ID) else {
        return Ok(());
    };
    let present = final_job
        .steps
        .iter()
        .any(|step| step.name == DOWNLOAD_PLAN_NAME);
    if !present {
        let at = final_download_at(final_job);
        final_job.steps.insert(at, download_plan_step()?);
    }
    Ok(())
}

/// Insert before the merge write-request, else before merge, else at end.
fn final_download_at(job: &Job) -> usize {
    let want = [steps::WRITE_REQUEST_OPERATION, steps::MERGE_OPERATION].join(":");
    let at = |op: &str| {
        job.steps.iter().position(
            |step| matches!(&step.kind, StepKind::Internal { operation } if operation == op),
        )
    };
    at(&want)
        .or_else(|| at(steps::MERGE_OPERATION))
        .unwrap_or(job.steps.len())
}

/// Insert freshness before and publish after the plan step, once each.
///
/// Anchored on the `plan-v1` step: without it there is no planner to
/// close over and the job is left untouched (the strict entrypoint
/// rejects anchorless plan jobs separately). Re-running never dupes.
/// # Errors
pub(crate) fn insert_plan_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let Some(plan) = jobs.get_mut(PLAN_JOB_ID) else {
        return Ok(());
    };
    let Some(at) = plan.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation } if operation == steps::PLAN_OPERATION)
    }) else {
        return Ok(());
    };
    if !plan
        .steps
        .iter()
        .any(|step| step.name == CHECK_GENERATED_NAME)
    {
        let check = freshness_step(&ctx.staged_binary, FRESHNESS_OUTDIR, &ctx.plan_consumer_env)?;
        plan.steps.insert(at, check);
    }
    let Some(at) = plan.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation } if operation == steps::PLAN_OPERATION)
    }) else {
        return Ok(());
    };
    if !plan.steps.iter().any(|step| step.name == PUBLISH_PLAN_NAME) {
        plan.steps.insert(at + 1, publish_plan_step()?);
    }
    Ok(())
}

/// Insert write-request steps immediately before their consumers.
///
/// The `plan-v1`/`merge-v1` gates require the request file materialized
/// (workflow-contract §3); the orchestrator emits these in IR, and this
/// backstop closes hand-built IR the same way. Idempotent: present
/// request steps are never duplicated, and insertion always lands after
/// any Acquire step (directly before the already-staged consumer).
/// # Errors
pub(crate) fn insert_request_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for (job_id, target) in [
        (PLAN_JOB_ID, steps::PLAN_OPERATION),
        (crate::render::FINAL_JOB_ID, steps::MERGE_OPERATION),
    ] {
        let Some(job) = jobs.get_mut(job_id) else {
            continue;
        };
        let want = format!("{}:{target}", steps::WRITE_REQUEST_OPERATION);
        let present = job.steps.iter().any(
            |step| matches!(&step.kind, StepKind::Internal { operation } if operation == &want),
        );
        if present {
            continue;
        }
        if let Some(at) = job.steps.iter().position(
            |step| matches!(&step.kind, StepKind::Internal { operation } if operation == target),
        ) {
            job.steps.insert(at, steps::write_request_step(target)?);
        }
    }
    Ok(())
}

/// Append the matrix-report upload to the task job exactly once.
///
/// Cache-contract §4 requires every matrix entry to upload its artifact
/// with `if: always()` (attached at render); the template carries the
/// leg's `matrix-report.json` plus `tasks/` files under the derived
/// `velnor-matrix-<run-key>-<matrix-key>` name. Idempotent.
/// # Errors
pub(crate) fn insert_task_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let Some(task) = jobs.get_mut(crate::render::TASK_JOB_ID) else {
        return Ok(());
    };
    let present = task
        .steps
        .iter()
        .any(|step| step.name == steps::MATRIX_REPORT_UPLOAD_NAME);
    if !present {
        task.steps.push(steps::matrix_report_upload_step()?);
    }
    Ok(())
}

/// Require the `plan-v1` anchor in the plan job (strict entrypoint).
/// # Errors
pub(crate) fn check_plan_anchor(
    jobs: &std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let anchored = jobs.get(PLAN_JOB_ID).is_some_and(|plan| {
        plan.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Internal { operation } if operation == steps::PLAN_OPERATION)
        })
    });
    if anchored || !jobs.contains_key(PLAN_JOB_ID) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "plan_job_without_plan_step".to_owned(),
        ))
    }
}
