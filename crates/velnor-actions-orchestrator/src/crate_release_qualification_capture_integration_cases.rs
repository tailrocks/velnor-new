// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn captures_coherent_evidence_and_rejects_mid_capture_source_change()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut api, candidate, source, witness) = fixture()?;
    let reviewers = BTreeSet::from([90]);
    let feedback = FeedbackContext {
        repository: &candidate.repository,
        pull_request_number: 7,
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
        candidate_feedback: &feedback,
    };
    let result = capture_rehearsal(&request, &api)?;
    assert_eq!(result.bundle, b"fixture-output");
    assert_eq!(result.metadata.source_sha(), "c".repeat(40));
    let mut wrong_candidate = candidate.clone();
    wrong_candidate.repository_id += 1;
    assert!(
        capture_rehearsal(
            &CaptureRequest {
                candidate: &wrong_candidate,
                ..request
            },
            &api
        )
        .is_err()
    );
    let wrong_reviewers = BTreeSet::from([91]);
    let wrong_feedback = FeedbackContext {
        expected_reviewer_app_ids: &wrong_reviewers,
        ..feedback
    };
    assert!(
        capture_rehearsal(
            &CaptureRequest {
                candidate_feedback: &wrong_feedback,
                ..request
            },
            &api
        )
        .is_err()
    );
    let original_log = api.log.clone();
    api.log = String::from_utf8(api.log)?
        .replace(
            &format!("tree={}", "d".repeat(40)),
            &format!("tree={}", "f".repeat(40)),
        )
        .into_bytes();
    assert!(capture_rehearsal(&request, &api).is_err());
    api.log = original_log;
    api.source_reads.set(0);
    api.mutate = true;
    assert!(capture_rehearsal(&request, &api).is_err());
    Ok(())
}
