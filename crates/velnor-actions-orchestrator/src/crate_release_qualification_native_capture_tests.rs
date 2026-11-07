// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    crate_release_github::NativeGitHubAdmissionRead,
    crate_release_qualification_capture::CaptureReviewProof,
};

#[path = "crate_release_qualification_native_server.rs"]
mod server;
use server::{Mutation, Server};

fn run(mutation: Mutation) -> Result<bool, Box<dyn std::error::Error>> {
    let (mut fixture, candidate, source, mut witness) = fixture()?;
    witness
        .metadata
        .as_mut()
        .ok_or("metadata")?
        .qualification
        .review_proof = velnor_actions_contract::config::ReleaseReviewProof::DirectCodex;
    add_workflow(&mut fixture.values, &witness)?;
    let server = Server::start(fixture, mutation)?;
    let api = NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&server.api)?,
    };
    let request = CaptureRequest {
        candidate: &candidate,
        source: &source,
        witness: &witness,
        workflow_default_branch: "main",
        review_proof: CaptureReviewProof::DirectCodex,
    };
    let result =
        crate::api::github_evidence_http::with_test_origins(&server.api, &server.storage, || {
            capture_rehearsal(&request, &api)
        });
    if mutation == Mutation::None {
        let captured = result?;
        assert_eq!(captured.bundle, b"fixture-output");
        assert_eq!(captured.metadata.source_sha(), "c".repeat(40));
        server.assert_authentication(true);
        return Ok(true);
    }
    server.assert_authentication(false);
    server.assert_mutation();
    Ok(result.is_ok())
}

#[test]
fn direct_capture_uses_native_http_and_rejects_mutable_or_untrusted_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(run(Mutation::None)?);
    for mutation in [
        Mutation::Repository,
        Mutation::App,
        Mutation::Feedback,
        Mutation::Source,
        Mutation::Checks,
        Mutation::Duplicate,
        Mutation::Pagination,
        Mutation::Redirect,
    ] {
        assert!(!run(mutation)?, "accepted {mutation:?}");
    }
    Ok(())
}

#[test]
fn direct_capture_rejects_wrong_proof_mode_before_http() -> Result<(), Box<dyn std::error::Error>> {
    let (mut fixture, candidate, source, mut witness) = fixture()?;
    witness
        .metadata
        .as_mut()
        .ok_or("metadata")?
        .qualification
        .review_proof = velnor_actions_contract::config::ReleaseReviewProof::DirectCodex;
    add_workflow(&mut fixture.values, &witness)?;
    let server = Server::start(fixture, Mutation::None)?;
    let api = NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&server.api)?,
    };
    let reviewers = BTreeSet::from([90]);
    let feedback = FeedbackContext {
        repository: &candidate.repository,
        pull_request_number: candidate.pull_request,
        head_sha: &candidate.head_sha,
        completion_app_id: 80,
        completion_check_name: "review-complete",
        expected_reviewer_app_ids: &reviewers,
    };
    let request = CaptureRequest {
        candidate: &candidate,
        source: &source,
        witness: &witness,
        workflow_default_branch: "main",
        review_proof: CaptureReviewProof::CompletionCheck(&feedback),
    };
    assert!(capture_rehearsal(&request, &api).is_err());
    Ok(())
}
