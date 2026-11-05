// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::crate_release_github::GitHubAdmissionRead;
use crate::github_review_feedback::FeedbackQuery;
use serde_json::Value;
use std::collections::BTreeSet;

struct NoReads;
impl GitHubAdmissionRead for NoReads {
    fn get(&self, _: &str) -> Result<Value, OrchestratorError> {
        panic!("unexpected GitHub request")
    }
    fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
        panic!("unexpected GitHub request")
    }
}
impl WitnessRead for NoReads {
    fn witness_job_log(&self, _: &str, _: u64) -> Result<Vec<u8>, OrchestratorError> {
        panic!("unexpected log request")
    }
    fn artifact_bytes(
        &self,
        _: &str,
        _: u64,
        _: u64,
        _: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        panic!("unexpected artifact request")
    }
}
impl FeedbackRead for NoReads {
    fn feedback_get(&self, _: &str) -> Result<Value, OrchestratorError> {
        panic!("unexpected feedback request")
    }
    fn feedback_graphql(
        &self,
        _: FeedbackQuery,
        _: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        panic!("unexpected GraphQL request")
    }
}
#[test]
fn rejects_source_from_another_repository_before_acquisition() {
    let head = "a".repeat(40);
    let candidate = ReviewedSourceRequest {
        repository: "candidate/repo".into(),
        repository_id: 1,
        pull_request: 1,
        head_sha: head.clone(),
        default_branch: "main".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        state: ReviewedPullState::OpenHead,
    };
    let source = ReviewedSourceRequest {
        repository: "consumer/repo".into(),
        repository_id: 2,
        pull_request: 2,
        head_sha: head.clone(),
        default_branch: "main".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        state: ReviewedPullState::Merged {
            merge_sha: head.clone(),
            merge_tree_sha: head.clone(),
        },
    };
    let witness = WitnessRequest {
        repository: "consumer/repo".into(),
        repository_id: 3,
        workflow_sha: head.clone(),
        workflow_path: ".github/workflows/release-rehearsal.yml".into(),
        run_id: 3,
        run_attempt: 1,
        job_id: 4,
        environment: "release".into(),
        reviewer_id: 5,
        output_bundle: true,
        metadata: Some(
            velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
                manifest_path: "Cargo.toml".into(),
                rust_version: "1.99.1".into(),
            },
        ),
        source_sha: Some(head.clone()),
        script: "review.sh".into(),
        source_sha256: "b".repeat(64),
        candidate_pr: 1,
        candidate_sha: head.clone(),
    };
    let reviewer_ids = BTreeSet::from([1]);
    let feedback = FeedbackContext {
        repository: &candidate.repository,
        pull_request_number: 1,
        head_sha: &head,
        completion_app_id: 1,
        completion_check_name: "complete",
        expected_reviewer_app_ids: &reviewer_ids,
    };
    let check = |source| {
        capture_rehearsal(
            &CaptureRequest {
                candidate: &candidate,
                source,
                witness: &witness,
                workflow_default_branch: "main",
                candidate_feedback: &feedback,
            },
            &NoReads,
        )
    };
    assert!(check(&source).is_err());
    let mut wrong_name = source.clone();
    wrong_name.repository_id = 3;
    wrong_name.repository = "other/repo".into();
    assert!(check(&wrong_name).is_err());
}

#[test]
fn metadata_must_match_admitted_source_tree_and_strict_json() {
    assert!(
        verify_metadata_source("source", "tree", br#"{"packages":[]}"#, "source", "tree").is_ok()
    );
    assert!(verify_metadata_source("other", "tree", br#"{}"#, "source", "tree").is_err());
    assert!(verify_metadata_source("source", "other", br#"{}"#, "source", "tree").is_err());
    assert!(
        verify_metadata_source("source", "tree", br#"{"x":1,"x":1}"#, "source", "tree").is_err()
    );
    assert!(verify_metadata_source("source", "tree", br#"[]"#, "source", "tree").is_err());
}
