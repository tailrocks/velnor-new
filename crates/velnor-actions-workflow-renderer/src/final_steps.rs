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

use velnor_actions_contract::{Job, Step};

use crate::{RenderError, render::FINAL_JOB_ID, steps};

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
/// and fetches each by exact name; absent artifacts stay absent and the
/// merge judges them honestly. `continue-on-error` attaches at render.
/// # Errors
pub(crate) fn fetch_reports_step() -> Result<Step, RenderError> {
    steps::internal_step(FETCH_REPORTS_NAME, steps::FETCH_OPERATION)
}

/// Final-report upload step (`velnor-final-<run-key>`, fails loud).
///
/// Uploads the single verdict file the merge step wrote; `if-no-files-found:
/// error` fails closed when the merge wrote nothing. `if: always()` is
/// attached at render.
/// # Errors
pub(crate) fn publish_final_report_step() -> Result<Step, RenderError> {
    steps::action_step(
        PUBLISH_FINAL_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), FINAL_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), FINAL_ARTIFACT_PATH.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
        ]),
    )
}

/// Insert the fan-in fetch and final publish into the final job.
///
/// The fetch lands after `Download plan` (it enumerates the plan) and
/// before the merge write-request; the publish closes the job after
/// `Merge reports`. Without a final job there is nothing to close over.
/// Re-running never dupes.
/// # Errors
pub(crate) fn insert_final_fanin(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(final_job) = jobs.get_mut(FINAL_JOB_ID) else {
        return Ok(());
    };
    if !final_job
        .steps
        .iter()
        .any(|step| step.name == FETCH_REPORTS_NAME)
    {
        let at = fetch_insert_at(final_job);
        final_job.steps.insert(at, fetch_reports_step()?);
    }
    if !final_job
        .steps
        .iter()
        .any(|step| step.name == PUBLISH_FINAL_NAME)
    {
        final_job.steps.push(publish_final_report_step()?);
    }
    Ok(())
}

/// Insert after `Download plan`, else before write-request/merge, else end.
fn fetch_insert_at(job: &Job) -> usize {
    use velnor_actions_contract::StepKind;
    if let Some(at) = job
        .steps
        .iter()
        .position(|step| step.name == crate::closure::DOWNLOAD_PLAN_NAME)
    {
        return at + 1;
    }
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
