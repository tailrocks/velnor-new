use super::*;
use super::{
    github::{GITHUB_ACTIONS_BOT_ID, ReleaseAuthor},
    identity::{MAX_RECOVERY_BODY_BYTES, make_recovery_body},
};

#[test]
fn recovery_envelope_binds_exact_receipt_and_signature_bundle() {
    let receipt = br#"{"run_id":17}"#;
    let bundle = br#"{"signature":"signed"}"#;
    let digest = sha256(receipt);
    let expected = make_recovery_body(receipt, bundle, &digest).expect("bounded envelope");
    let author = ReleaseAuthor {
        login: Some("github-actions[bot]".to_owned()),
        id: Some(GITHUB_ACTIONS_BOT_ID),
    };
    assert_eq!(
        draft_receipt_digest(Some(&expected), Some(&author), &expected, &digest,).as_deref(),
        Some(digest.as_str())
    );
    let replayed_identical_candidate =
        make_recovery_body(receipt, bundle, &digest).expect("identical original envelope");
    assert_eq!(replayed_identical_candidate, expected);

    let changed_receipt = br#"{"run_id":18}"#;
    let changed_digest = sha256(changed_receipt);
    let other_attempt = make_recovery_body(changed_receipt, bundle, &changed_digest)
        .expect("other signed attempt envelope");
    assert_ne!(
        draft_receipt_digest(Some(&other_attempt), Some(&author), &expected, &digest,).as_deref(),
        Some(digest.as_str())
    );
    assert_ne!(
        draft_receipt_digest(Some("malformed"), Some(&author), &expected, &digest).as_deref(),
        Some(digest.as_str())
    );
    assert_eq!(
        draft_receipt_digest(Some(&expected), None, &expected, &digest),
        None
    );
    assert_eq!(
        tag_receipt_digest(Some(&expected), &expected, &digest).as_deref(),
        Some(digest.as_str())
    );
    assert_ne!(
        tag_receipt_digest(Some("different envelope"), &expected, &digest).as_deref(),
        Some(digest.as_str())
    );
    let forged_author = ReleaseAuthor {
        login: Some("untrusted".to_owned()),
        id: Some(1),
    };
    assert_ne!(
        draft_receipt_digest(Some(&expected), Some(&forged_author), &expected, &digest,).as_deref(),
        Some(digest.as_str())
    );
}

#[test]
fn recovery_envelope_size_is_checked_before_any_remote_write() {
    let huge = vec![b'x'; MAX_RECOVERY_BODY_BYTES];
    let digest = sha256(&huge);
    assert!(make_recovery_body(&huge, b"bundle", &digest).is_err());
}

#[test]
fn publisher_ci_gate_binds_exact_run_and_attempt() {
    let run = WorkflowRunResponse {
        id: Some(22),
        run_attempt: Some(3),
        path: None,
        head_sha: Some("a".repeat(40)),
        head_branch: Some("main".to_owned()),
        event: Some("push".to_owned()),
        status: Some("completed".to_owned()),
        conclusion: Some("success".to_owned()),
        repository: Some(RunRepository { id: 9 }),
    };
    assert!(ci_run_is_success(&run, 22, 3, 9, &"a".repeat(40), "main"));
    assert!(!ci_run_is_success(&run, 22, 2, 9, &"a".repeat(40), "main"));
    assert!(!ci_run_is_success(&run, 21, 3, 9, &"a".repeat(40), "main"));
}

#[test]
fn stable_and_preview_publications_never_request_latest() {
    assert!(!make_latest(PackageReleaseChannel::Stable));
    assert!(!make_latest(PackageReleaseChannel::Preview));
}
