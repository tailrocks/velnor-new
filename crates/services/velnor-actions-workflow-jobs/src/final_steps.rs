//! Final-job fan-in: exact report fetch plus the final-report publish.
//!
//! Workflow-contract §3 fixes the final steps: `Download plan`, `Download
//! every expected matrix artifact`, `Merge reports`, `Publish final
//! report`. The plan download lives in `closure`; this module owns the
//! fetch step (a fixed internal op: the helper downloads each
//! plan-expected artifact by exact derived name with `gh`, never a
//! `pattern:` wildcard) and the `velnor-final-<run-key>` upload
//! (`if: always()` attaches at render, like every upload).

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, Step, StepRole};

use crate::context::{CANDIDATE_JOB_ID, FINAL_JOB_ID, RenderContext};
use velnor_actions_workflow_steps::{RenderError, steps};

/// Contract-fixed display name of the matrix fan-in fetch step.
pub(crate) const FETCH_REPORTS_NAME: &str = "Download every expected matrix artifact";
/// Contract-fixed display name of the final-report upload step.
pub(crate) const PUBLISH_FINAL_NAME: &str = "Publish final report";
/// Final artifact name with the derived run key (contract §4).
pub(crate) const FINAL_ARTIFACT_NAME: &str =
    "velnor-final-r${{ github.run_id }}-a${{ github.run_attempt }}";
/// Final verdict file the merge step writes (contract §3).
pub(crate) const FINAL_ARTIFACT_PATH: &str =
    "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ github.run_attempt }}/final-report.json";

/// Matrix fan-in fetch step (fixed internal `fetch-reports-v1` op).
///
/// The helper enumerates expected artifact IDs from the downloaded plan
/// and fetches each by exact name with bounded per-leg retry; absent
/// artifacts stay absent and the merge judges them honestly. No
/// `continue-on-error` (F5): hard failures fail the job, unmasked.
/// # Errors
pub(crate) fn fetch_reports_step() -> Result<Step, RenderError> {
    let mut step = steps::internal_step(FETCH_REPORTS_NAME, steps::FETCH_OPERATION)?;
    step.role = Some(StepRole::FetchReports);
    Ok(step)
}

/// Final-report upload step (`velnor-final-<run-key>`, fails loud).
///
/// Uploads the single verdict file the merge step wrote; `if-no-files-found:
/// error` fails closed when the merge wrote nothing. `if: always()` is
/// attached at render.
/// # Errors
pub(crate) fn publish_final_report_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        PUBLISH_FINAL_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), FINAL_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), FINAL_ARTIFACT_PATH.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::PublishFinal);
    Ok(step)
}

/// Insert the fan-in fetch and final publish into the final job.
///
/// The fetch lands after `Download plan` (it enumerates the plan) and
/// before the merge write-request; the publish closes the job after
/// `Merge reports`. In candidate mode the candidate artifact downloads
/// beside the fetch so the merge can re-check the head-bound
/// attestation. Without a final job there is nothing to close over.
/// Re-running never dupes.
/// # Errors
pub fn insert_final_fanin(
    jobs: &mut BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let candidate = jobs.contains_key(CANDIDATE_JOB_ID);
    let Some(final_job) = jobs.get_mut(FINAL_JOB_ID) else {
        return Ok(());
    };
    if candidate
        && !final_job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::AttestationDownload))
    {
        let at = fetch_insert_at(final_job);
        final_job.steps.insert(at, download_attestation_step(ctx)?);
    }
    if !final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::FetchReports))
    {
        let at = fetch_insert_at(final_job);
        final_job.steps.insert(at, fetch_reports_step()?);
    }
    if !final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::PublishFinal))
    {
        final_job.steps.push(publish_final_report_step()?);
    }
    Ok(())
}

/// Contract-fixed display name of the candidate-artifact download step.
pub(crate) const ATTESTATION_DOWNLOAD_NAME: &str = "Download candidate attestation";

/// Candidate-artifact download step for the head-bound attestation.
///
/// Downloads the whole candidate artifact (binary, manifest, plus the
/// attestation the candidate job wrote beside the manifest) into the
/// run-key directory under the evidence subdir, before the merge
/// write-request assembles it. The single-label workflow invariant
/// makes the artifact name derivable from the context runs-on.
/// # Errors
fn download_attestation_step(ctx: &RenderContext) -> Result<Step, RenderError> {
    let target = velnor_actions_contract_release::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let artifact = crate::candidate::candidate_artifact_name(target.triple())?;
    let path = format!(
        "{}/{}",
        crate::closure::PLAN_ARTIFACT_PATH,
        velnor_actions_contract_workflow::CANDIDATE_EVIDENCE_SUBDIR
    );
    let mut step = steps::download_artifact_step(&artifact, &path)?;
    ATTESTATION_DOWNLOAD_NAME.clone_into(&mut step.name);
    step.role = Some(StepRole::AttestationDownload);
    Ok(step)
}

/// Insert after `Download plan`, else before write-request/merge, else end.
fn fetch_insert_at(job: &Job) -> usize {
    use velnor_actions_contract_workflow::StepKind;
    if let Some(at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::DownloadPlan))
    {
        return at + 1;
    }
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
