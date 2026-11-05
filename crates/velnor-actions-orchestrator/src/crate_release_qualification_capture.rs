// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_qualification_source::{ReviewedPullState, ReviewedSourceRequest};
use crate::crate_release_qualification_witness::{WitnessRead, WitnessRequest};
use crate::github_review_feedback::{FeedbackContext, FeedbackRead};
use crate::{OrchestratorError, internal::internal};

pub trait QualificationRead: WitnessRead + FeedbackRead {}
impl<T: WitnessRead + FeedbackRead> QualificationRead for T {}

#[derive(Debug)]
pub struct CaptureRequest<'a> {
    pub candidate: &'a ReviewedSourceRequest,
    pub source: &'a ReviewedSourceRequest,
    pub witness: &'a WitnessRequest,
    pub workflow_default_branch: &'a str,
    pub candidate_feedback: &'a FeedbackContext<'a>,
}

pub struct CapturedRehearsal {
    pub bundle: Vec<u8>,
}

impl std::fmt::Debug for CapturedRehearsal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedRehearsal")
            .field("bundle_bytes", &self.bundle.len())
            .finish()
    }
}

pub fn capture_rehearsal(
    request: &CaptureRequest<'_>,
    api: &dyn QualificationRead,
) -> Result<CapturedRehearsal, OrchestratorError> {
    let candidate = request.candidate;
    let source = request.source;
    let witness = request.witness;
    let feedback = request.candidate_feedback;
    if !matches!(candidate.state, ReviewedPullState::OpenHead)
        || !matches!(source.state, ReviewedPullState::Merged { .. })
        || candidate.pull_request != witness.candidate_pr
        || candidate.head_sha != witness.candidate_sha
        || source.repository != witness.repository
        || source.repository_id != witness.repository_id
        || feedback.repository != candidate.repository
        || feedback.pull_request_number != candidate.pull_request
        || feedback.head_sha != candidate.head_sha
        || !witness.output_bundle
    {
        return Err(internal("qualification_capture_identity"));
    }
    let before_candidate =
        crate::crate_release_qualification_source::verify_pull_source(candidate, api)?;
    let before_source = crate::crate_release_qualification_source::verify_pull_source(source, api)?;
    let before_feedback =
        crate::crate_release_qualification_checks::verify_refreshed_feedback(api, api, feedback)?;
    crate::crate_release_qualification_approval::verify_rehearsal_approval(
        witness,
        request.workflow_default_branch,
        api,
    )?;
    let before_workflow = crate::crate_release_qualification_workflow::verify_canonical_rehearsal(
        witness,
        request.workflow_default_branch,
        api,
    )?;
    let before_witness = crate::crate_release_qualification_witness::verify_witness(witness, api)?;
    let bundle = crate::crate_release_qualification_bundle::acquire_output_bundle(witness, api)?;
    if before_candidate
        != crate::crate_release_qualification_source::verify_pull_source(candidate, api)?
        || before_source
            != crate::crate_release_qualification_source::verify_pull_source(source, api)?
        || before_feedback
            != crate::crate_release_qualification_checks::verify_refreshed_feedback(
                api, api, feedback,
            )?
        || before_workflow
            != crate::crate_release_qualification_workflow::verify_canonical_rehearsal(
                witness,
                request.workflow_default_branch,
                api,
            )?
        || before_witness
            != crate::crate_release_qualification_witness::verify_witness(witness, api)?
    {
        return Err(internal("qualification_capture_changed"));
    }
    crate::crate_release_qualification_approval::verify_rehearsal_approval(
        witness,
        request.workflow_default_branch,
        api,
    )?;
    Ok(CapturedRehearsal { bundle })
}

#[cfg(test)]
#[path = "crate_release_qualification_capture_tests.rs"]
mod tests;
