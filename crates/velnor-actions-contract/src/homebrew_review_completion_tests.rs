use super::*;

fn fixture() -> (TapCompletionCheckMetadata, TapReviewCompletionPayload) {
    let head = "a".repeat(40);
    (
        TapCompletionCheckMetadata {
            name: "Review Complete".to_owned(),
            app_id: 2,
            head_sha: head.clone(),
            completed_successfully: true,
        },
        TapReviewCompletionPayload {
            schema: TAP_REVIEW_COMPLETION_SCHEMA.to_owned(),
            repository: "example/tap".to_owned(),
            pull_request_number: 42,
            head_sha: head.clone(),
            verdict: TapReviewVerdict::Complete,
            feedback_digest: format!("b3:{}", "b".repeat(64)),
            has_unaddressed_findings: false,
            reviewers: vec![TapCompletedReviewer {
                app_id: 3,
                head_sha: head,
                evidence_digest: format!("b3:{}", "c".repeat(64)),
                complete: true,
                has_unaddressed_findings: false,
            }],
        },
    )
}

#[test]
fn canonical_complete_payload_preserves_independently_authenticated_metadata() {
    let (metadata, payload) = fixture();
    let parsed =
        parse_tap_review_completion(&metadata, &canonical_json_bytes(&payload).expect("bytes"))
            .expect("complete");
    assert_eq!(parsed.app_id, 2);
    assert_eq!(parsed.completed_reviewer_app_ids, BTreeSet::from([3]));
    assert_eq!(parsed.pull_request_number, 42);
}

#[test]
fn unknown_schema_fields_duplicate_keys_and_noncanonical_text_are_rejected() {
    let (metadata, payload) = fixture();
    let original = String::from_utf8(canonical_json_bytes(&payload).expect("bytes")).expect("utf8");
    for text in [
        format!(" {original}"),
        original.replacen('{', "{\"schema\":\"forged\",", 1),
        original.replacen('{', "{\"ignored\":true,", 1),
        original.replace(TAP_REVIEW_COMPLETION_SCHEMA, "velnor.review-completion.v2"),
        original.replace("\"complete\"", "\"failed\""),
    ] {
        assert!(parse_tap_review_completion(&metadata, text.as_bytes()).is_err());
    }
}

#[test]
fn missing_stale_or_unfinished_reviewer_cannot_become_completion() {
    for change in 0..7 {
        let (metadata, mut payload) = fixture();
        match change {
            0 => payload.reviewers.clear(),
            1 => payload.reviewers[0].complete = false,
            2 => payload.reviewers[0].has_unaddressed_findings = true,
            3 => payload.reviewers[0].head_sha = "d".repeat(40),
            4 => payload.has_unaddressed_findings = true,
            5 => payload.reviewers.push(payload.reviewers[0].clone()),
            _ => payload.reviewers[0].evidence_digest = "unverified".to_owned(),
        }
        assert!(
            parse_tap_review_completion(&metadata, &canonical_json_bytes(&payload).expect("bytes"))
                .is_err(),
            "change {change}"
        );
    }
}

#[test]
fn payload_cannot_override_failed_or_stale_check_metadata() {
    let (mut metadata, payload) = fixture();
    let bytes = canonical_json_bytes(&payload).expect("bytes");
    metadata.completed_successfully = false;
    assert!(parse_tap_review_completion(&metadata, &bytes).is_err());
    metadata.completed_successfully = true;
    metadata.head_sha = "d".repeat(40);
    assert!(parse_tap_review_completion(&metadata, &bytes).is_err());
}
