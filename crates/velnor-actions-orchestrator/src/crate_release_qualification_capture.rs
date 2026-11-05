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
    pub metadata: crate::crate_release_qualification_bundle::MetadataEvidence,
}

impl std::fmt::Debug for CapturedRehearsal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedRehearsal")
            .field("bundle_bytes", &self.bundle.len())
            .field("metadata_bytes", &self.metadata.bytes().len())
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
    validate_capture_identity(request)?;
    let ReviewedPullState::Merged {
        merge_sha,
        merge_tree_sha,
    } = &source.state
    else {
        return Err(internal("qualification_capture_identity"));
    };
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
    let metadata = crate::crate_release_qualification_bundle::acquire_metadata(witness, api)?;
    verify_metadata_source(
        metadata.source_sha(),
        metadata.source_tree_sha(),
        metadata.bytes(),
        merge_sha,
        merge_tree_sha,
    )?;
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
    Ok(CapturedRehearsal { bundle, metadata })
}

fn validate_capture_identity(request: &CaptureRequest<'_>) -> Result<(), OrchestratorError> {
    let candidate = request.candidate;
    let source = request.source;
    let witness = request.witness;
    let feedback = request.candidate_feedback;
    let policy = &witness
        .metadata
        .as_ref()
        .ok_or_else(|| internal("qualification_capture_identity"))?
        .qualification;
    let ReviewedPullState::Merged { merge_sha, .. } = &source.state else {
        return Err(internal("qualification_capture_identity"));
    };
    if !matches!(candidate.state, ReviewedPullState::OpenHead)
        || candidate.pull_request != witness.candidate_pr
        || candidate.head_sha != witness.candidate_sha
        || candidate.repository != policy.candidate_repository
        || candidate.repository_id != policy.candidate_repository_id
        || candidate.default_branch != policy.candidate_default_branch
        || candidate.required_workflow_path != policy.candidate_required_workflow_path
        || source.repository != witness.repository
        || source.repository_id != witness.repository_id
        || source.default_branch != request.workflow_default_branch
        || feedback.repository != candidate.repository
        || feedback.pull_request_number != candidate.pull_request
        || feedback.head_sha != candidate.head_sha
        || feedback.completion_app_id != policy.completion_app_id
        || feedback.completion_check_name != policy.completion_check_name
        || feedback.expected_reviewer_app_ids != &policy.reviewer_app_ids
        || !witness.output_bundle
        || witness.source_sha.as_deref() != Some(merge_sha)
    {
        return Err(internal("qualification_capture_identity"));
    }
    Ok(())
}

fn verify_metadata_source(
    source: &str,
    tree: &str,
    bytes: &[u8],
    expected_source: &str,
    expected_tree: &str,
) -> Result<(), OrchestratorError> {
    if source != expected_source || tree != expected_tree {
        return Err(internal("qualification_metadata_source_mismatch"));
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, 16 * 1024 * 1024)
            .map_err(|_| internal("qualification_metadata_invalid"))?;
    if !value.is_object() {
        return Err(internal("qualification_metadata_invalid"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "crate_release_qualification_capture_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "crate_release_qualification_capture_integration_tests.rs"]
mod integration_tests;
