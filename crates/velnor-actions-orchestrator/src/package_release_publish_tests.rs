use super::*;
use super::{
    github::sha256,
    github::{
        GITHUB_ACTIONS_BOT_ID, ReleaseAuthor, RunRepository, WorkflowMetadata, WorkflowRunResponse,
    },
    identity::{MAX_RECOVERY_BODY_BYTES, make_recovery_body},
    run_identity::RunSource,
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
fn same_source_new_preview_attempt_cannot_resume_another_signed_envelope() {
    use std::collections::{BTreeMap, BTreeSet};
    use velnor_actions_contract::publication_retry::{
        PublicationAction, PublicationIdentity, PublicationState, publication_action,
    };

    let source_sha = "a".repeat(40);
    let assets = BTreeMap::from([("release.json".to_owned(), "c".repeat(64))]);
    let attempt_a = PublicationIdentity {
        source_sha: source_sha.clone(),
        receipt_digest: "b".repeat(64),
        assets: assets.clone(),
    };
    let attempt_b = PublicationIdentity {
        source_sha: source_sha.clone(),
        receipt_digest: "d".repeat(64),
        assets: assets.clone(),
    };
    let retained_tag = PublicationState::TagOnly {
        source_sha,
        original_receipt_digest: Some(attempt_a.receipt_digest.clone()),
    };
    let available = BTreeSet::from(["release.json".to_owned()]);

    assert_eq!(
        publication_action(&attempt_b, &retained_tag, true, &available),
        PublicationAction::Reject(
            velnor_actions_contract::publication_retry::PublicationRejection::ConflictingIdentity
        )
    );
    assert_eq!(
        publication_action(&attempt_a, &retained_tag, true, &available),
        PublicationAction::CreateDraft,
        "recovery retains original A bytes and envelope"
    );
}

#[test]
fn publisher_ci_gate_binds_exact_run_and_attempt() {
    let run = WorkflowRunResponse {
        id: Some(22),
        run_attempt: Some(3),
        path: Some(velnor_actions_contract::CI_WORKFLOW_PATH.to_owned()),
        name: Some(velnor_actions_contract::WORKFLOW_DISPLAY_NAME.to_owned()),
        workflow_id: Some(7),
        run_number: Some(12),
        head_repository: Some(RunRepository { id: 9 }),
        head_sha: Some("a".repeat(40)),
        head_branch: Some("main".to_owned()),
        event: Some("push".to_owned()),
        status: Some("completed".to_owned()),
        conclusion: Some("success".to_owned()),
        repository: Some(RunRepository { id: 9 }),
    };
    let workflow = WorkflowMetadata {
        state: "active".to_owned(),
        id: 7,
        path: velnor_actions_contract::CI_WORKFLOW_PATH.to_owned(),
        name: velnor_actions_contract::WORKFLOW_DISPLAY_NAME.to_owned(),
    };
    let sha = "a".repeat(40);
    let source = RunSource {
        repository_id: 9,
        sha: &sha,
        branch: "main",
    };
    assert!(run_identity::ci_run_is_success(
        &run, 22, 3, source, &workflow
    ));
    assert!(!run_identity::ci_run_is_success(
        &run, 22, 2, source, &workflow
    ));
    assert!(!run_identity::ci_run_is_success(
        &run, 21, 3, source, &workflow
    ));
}

#[test]
fn stable_and_preview_publications_never_request_latest() {
    assert!(!make_latest(PackageReleaseChannel::Stable));
    assert!(!make_latest(PackageReleaseChannel::Preview));
}

#[test]
fn release_json_lifecycle_admission_preserves_channel_prerelease_state() {
    use serde_json::{Value, json};
    let draft = json!({"id":42,"tag_name":"v1.2.3","draft":true,"prerelease":false,"immutable":false,"assets":[]});
    let parse =
        |value| serde_json::from_value::<ReleaseResponse>(value).expect("actual release response");
    admit_release_lifecycle(
        &parse(draft.clone()),
        "v1.2.3",
        PackageReleaseChannel::Stable,
    )
    .expect("native mutable stable draft");
    for (field, replacement) in [
        ("id", json!(0)),
        ("tag_name", json!("preview")),
        ("draft", Value::Null),
        ("prerelease", json!(true)),
        ("prerelease", Value::Null),
        ("immutable", json!(true)),
        ("immutable", Value::Null),
    ] {
        let mut changed = draft.clone();
        changed[field] = replacement;
        assert!(
            admit_release_lifecycle(&parse(changed), "v1.2.3", PackageReleaseChannel::Stable)
                .is_err(),
            "{field}"
        );
    }
    for missing in ["draft", "prerelease", "immutable"] {
        let mut changed = draft.clone();
        changed.as_object_mut().expect("object").remove(missing);
        assert!(
            admit_release_lifecycle(&parse(changed), "v1.2.3", PackageReleaseChannel::Stable)
                .is_err(),
            "missing {missing}"
        );
    }
    let mut published = draft;
    published["draft"] = json!(false);
    published["immutable"] = json!(true);
    admit_release_lifecycle(
        &parse(published.clone()),
        "v1.2.3",
        PackageReleaseChannel::Stable,
    )
    .expect("immutable stable publication");
    for immutable in [json!(false), Value::Null] {
        published["immutable"] = immutable;
        let response = parse(published.clone());
        admit_release_lifecycle(&response, "v1.2.3", PackageReleaseChannel::Stable)
            .expect("published planner owns immutability verdict");
        assert_ne!(response.immutable, Some(true));
    }

    let preview_draft = json!({"id":43,"tag_name":"preview-abc","draft":true,"prerelease":true,"immutable":false,"assets":[]});
    admit_release_lifecycle(
        &parse(preview_draft.clone()),
        "preview-abc",
        PackageReleaseChannel::Preview,
    )
    .expect("preview remains a prerelease draft");
    let mut invalid_preview = preview_draft;
    invalid_preview["prerelease"] = json!(false);
    assert!(
        admit_release_lifecycle(
            &parse(invalid_preview),
            "preview-abc",
            PackageReleaseChannel::Preview
        )
        .is_err()
    );
}
