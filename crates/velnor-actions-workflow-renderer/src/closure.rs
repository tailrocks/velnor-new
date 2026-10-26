// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{
    Job, RELEASE_MANIFEST_FILENAME, ReleaseManifest, ReleaseTarget, Step, StepKind, StepRole,
    check_release_artifact,
};

use crate::{
    RenderError, preseed,
    render::{PLAN_JOB_ID, RenderContext},
    steps,
};

#[path = "closure_freshness.rs"]
mod freshness_impl;
pub use freshness_impl::{freshness_step, freshness_step_with_claude_mode};

pub const CHECK_GENERATED_NAME: &str = "Check generated files";
pub const PUBLISH_PLAN_NAME: &str = "Publish plan";
pub const DOWNLOAD_PLAN_NAME: &str = "Download plan";
pub const FRESHNESS_OUTDIR: &str =
    "$RUNNER_TEMP/velnor-actions-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}";
pub const PLAN_ARTIFACT_NAME: &str =
    "velnor-plan-r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}";
pub const PLAN_ARTIFACT_PATH: &str =
    "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}";
pub const SEED_REMEDIATION: &str = "seed_required:publish the bootstrap release (manual pinned-Mise+MBX seed build), record it in .velnor/generator.lock, then regenerate";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperProvenance {
    ReleaseAsset {
        url: String,
        sha256: String,
        commit: String,
    },
    SeedRequired,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapControllerPin {
    pub version: String,
    pub target_triple: String,
    pub url: String,
    pub sha256: String,
    pub commit: String,
}

impl BootstrapControllerPin {
    pub fn from_release_manifest(
        manifest: &ReleaseManifest,
        runner_label: &str,
        expected_version: &str,
    ) -> Result<Self, RenderError> {
        manifest
            .validate(RELEASE_MANIFEST_FILENAME)
            .map_err(RenderError::Contract)?;
        let target = ReleaseTarget::for_runner_label(runner_label)
            .ok_or_else(|| RenderError::InvalidWorkflow("bootstrap_runner_target".to_owned()))?;
        if manifest.version != expected_version {
            return Err(RenderError::InvalidWorkflow(
                "bootstrap_descriptor_version".to_owned(),
            ));
        }
        if expected_version != env!("CARGO_PKG_VERSION") {
            return Err(RenderError::InvalidWorkflow(
                "bootstrap_generator_version".to_owned(),
            ));
        }
        let record = manifest.record_for_target(target.triple()).ok_or_else(|| {
            RenderError::InvalidWorkflow("bootstrap_descriptor_target".to_owned())
        })?;
        check_release_artifact(
            &record.artifact,
            &manifest.version,
            &manifest.commit,
            target.triple(),
            RELEASE_MANIFEST_FILENAME,
            "targets.artifact",
        )
        .map_err(RenderError::Contract)?;
        Ok(Self {
            version: manifest.version.clone(),
            target_triple: target.triple().to_owned(),
            url: record.artifact.clone(),
            sha256: record.sha256.clone(),
            commit: manifest.commit.clone(),
        })
    }
}

pub fn provision_bootstrap_acquire_steps(
    pin: &BootstrapControllerPin,
    provenance: &HelperProvenance,
    acquire_argv: &[String],
    bridge: &steps::BootstrapBridge,
) -> Result<Vec<Step>, RenderError> {
    let (url, sha256, commit) = match provenance {
        HelperProvenance::ReleaseAsset {
            url,
            sha256,
            commit,
        } => (url, sha256, commit),
        HelperProvenance::SeedRequired => {
            return Err(RenderError::InvalidWorkflow(SEED_REMEDIATION.to_owned()));
        }
    };
    if url != &pin.url || sha256 != &pin.sha256 || commit != &pin.commit {
        return Err(RenderError::InvalidWorkflow(
            "bootstrap_pin_provenance_mismatch".to_owned(),
        ));
    }
    let bridge_step = steps::bootstrap_bridge_step(bridge, &pin.sha256)?;
    if !acquire_argv
        .iter()
        .any(|arg| arg.contains(&bridge.staged_path))
    {
        return Err(RenderError::InvalidWorkflow(
            "bootstrap_staged_mismatch".to_owned(),
        ));
    }
    let marker = steps::bootstrap_marker_for(&bridge.staged_path);
    let guarded = steps::bootstrap_guarded_acquire_argv(&marker, acquire_argv)?;
    let acquire = provision_acquire_step(provenance, guarded)?;
    Ok(vec![bridge_step, acquire])
}

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
        } else if step.role == Some(StepRole::PreseedStage) {
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

pub(crate) fn is_acquire_step(step: &Step) -> bool {
    step.role == Some(StepRole::AcquireVelnor)
        && matches!(&step.kind, StepKind::Shell { env, .. } if env.contains_key(steps::ASSET_SHA_ENV) && env.contains_key(steps::ASSET_URL_ENV))
}

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

pub fn publish_plan_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
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
    )?;
    step.role = Some(StepRole::PublishPlan);
    Ok(step)
}

pub fn download_plan_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        DOWNLOAD_PLAN_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PLAN_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PLAN_ARTIFACT_PATH.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::DownloadPlan);
    Ok(step)
}

pub(crate) fn insert_final_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let Some(final_job) = jobs.get_mut(crate::render::FINAL_JOB_ID) else {
        return Ok(());
    };
    let present = final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::DownloadPlan));
    if !present {
        let at = final_download_at(final_job);
        final_job.steps.insert(at, download_plan_step()?);
    }
    Ok(())
}

fn final_download_at(job: &Job) -> usize {
    let want = [steps::WRITE_REQUEST_OPERATION, steps::MERGE_OPERATION].join(":");
    let at = |op: &str| {
        job.steps.iter().position(
            |step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == op),
        )
    };
    at(&want)
        .or_else(|| at(steps::MERGE_OPERATION))
        .unwrap_or(job.steps.len())
}

pub(crate) fn insert_plan_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let Some(plan) = jobs.get_mut(PLAN_JOB_ID) else {
        return Ok(());
    };
    let Some(at) = plan.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation, .. } if operation == steps::PLAN_OPERATION)
    }) else {
        return Ok(());
    };
    if !plan
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::CheckGenerated))
    {
        let check = freshness_step(&ctx.staged_binary, FRESHNESS_OUTDIR, &ctx.plan_consumer_env)?;
        plan.steps.insert(at, check);
    }
    let Some(at) = plan.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation, .. } if operation == steps::PLAN_OPERATION)
    }) else {
        return Ok(());
    };
    if !plan
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::PublishPlan))
    {
        plan.steps.insert(at + 1, publish_plan_step()?);
    }
    Ok(())
}

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
            |step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == &want),
        );
        if present {
            continue;
        }
        if let Some(at) = job.steps.iter().position(
            |step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == target),
        ) {
            job.steps.insert(at, steps::write_request_step(target)?);
        }
    }
    Ok(())
}

pub(crate) fn insert_task_closure(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let Some(task) = jobs.get_mut(crate::render::TASK_JOB_ID) else {
        return Ok(());
    };
    let present = task
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::MatrixReportUpload));
    if !present {
        task.steps.push(steps::matrix_report_upload_step()?);
    }
    Ok(())
}

pub(crate) fn check_plan_anchor(
    jobs: &std::collections::BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let anchored = jobs.get(PLAN_JOB_ID).is_some_and(|plan| {
        plan.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Internal { operation, .. } if operation == steps::PLAN_OPERATION)
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
