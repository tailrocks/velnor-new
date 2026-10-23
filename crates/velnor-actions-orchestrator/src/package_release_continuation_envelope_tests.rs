use super::*;
use serde_json::{Value, json};
use velnor_actions_contract::package_release::PackageReleaseEvent;

fn fixture() -> (PackageReleasePlan, String, Value) {
    let fixture = crate::tap_test_support::fixture();
    let mut plan = fixture.plan;
    plan.channel = PackageReleaseChannel::Stable;
    let mut receipt = fixture.release.receipt;
    receipt.channel = PackageReleaseChannel::Stable;
    receipt.source.event = PackageReleaseEvent::WorkflowDispatch;
    receipt.version.value = receipt.version.base.clone();
    receipt.version.commit_count = None;
    receipt.version.source_sha7 = None;
    receipt.tag = format!("v{}", receipt.version.base);
    receipt.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.workflow.signer_job_id = PackageReleaseChannel::Stable.signer_job_id().to_owned();
    receipt.workflow.signer_workflow = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        receipt.repository.slug, receipt.workflow.path, receipt.workflow.workflow_ref,
    );
    receipt.validate().expect("stable receipt");
    let bytes = receipt.canonical_bytes().expect("canonical receipt");
    let digest = sha256(&bytes);
    let body = make_recovery_body(&bytes, b"untrusted-signature-hint", &digest)
        .expect("canonical discovery envelope");
    let release = json!({"id":12,"tag_name":receipt.tag,"draft":false,"prerelease":false,
        "immutable":true,"body":body,"author":{"id":41_898_282,"login":"github-actions[bot]"},"assets":[]});
    (plan, body, release)
}

fn preview_fixture() -> (PackageReleasePlan, String, Value, String, u64) {
    let fixture = crate::tap_test_support::fixture();
    let plan = fixture.plan;
    let receipt = fixture.release.receipt;
    let source_sha = receipt.source.commit_sha.clone();
    let commit_count = receipt.version.commit_count.expect("preview count");
    let bytes = receipt.canonical_bytes().expect("canonical receipt");
    let digest = sha256(&bytes);
    let body = make_recovery_body(&bytes, b"untrusted-signature-hint", &digest)
        .expect("canonical discovery envelope");
    let release = json!({"id":13,"tag_name":receipt.tag,"draft":false,"prerelease":true,
        "immutable":true,"body":body,"author":{"id":41_898_282,"login":"github-actions[bot]"},"assets":[]});
    (plan, body, release, source_sha, commit_count)
}

#[test]
fn continuation_immutable_preview_envelope_yields_exact_source_run_hints() {
    let (plan, body, release, source_sha, count) = preview_fixture();
    let parsed = serde_json::from_value(release).expect("release response");
    let receipt = preview_receipt_hint(&parsed, &plan, 41, &source_sha, count, &body)
        .expect("discovery hint");
    assert_eq!(receipt.source.commit_sha, source_sha);
    assert_eq!(receipt.version.commit_count, Some(count));
    assert_eq!(receipt.tag, format!("preview-{source_sha}"));
    assert_eq!(
        (receipt.workflow.run_id, receipt.workflow.run_attempt),
        (8, 1)
    );
}

#[test]
fn continuation_preview_hint_rejects_rolling_or_foreign_publication_state() {
    let (plan, body, release, source_sha, count) = preview_fixture();
    for (pointer, value) in [
        ("/immutable", json!(false)),
        ("/draft", json!(true)),
        ("/prerelease", json!(false)),
        ("/author/id", json!(1)),
        ("/author/login", json!("other")),
        ("/tag_name", json!("preview")),
        ("/body", json!(format!("{body}extra\n"))),
    ] {
        let mut changed = release.clone();
        *changed.pointer_mut(pointer).expect("existing field") = value;
        let parsed = serde_json::from_value(changed).expect("release response");
        assert!(
            preview_receipt_hint(&parsed, &plan, 41, &source_sha, count, &body).is_err(),
            "{pointer}"
        );
    }
    let parsed = serde_json::from_value(release).expect("release response");
    assert!(preview_receipt_hint(&parsed, &plan, 42, &source_sha, count, &body).is_err());
    assert!(preview_receipt_hint(&parsed, &plan, 41, &"b".repeat(40), count, &body).is_err());
    assert!(preview_receipt_hint(&parsed, &plan, 41, &source_sha, count + 1, &body).is_err());
}

#[test]
fn continuation_canonical_envelope_yields_only_stable_original_run_hints() {
    let (plan, body, release) = fixture();
    let parsed = serde_json::from_value(release).expect("release response");
    let receipt = stable_receipt_hint(&parsed, &plan, 41, &body).expect("discovery hint");
    assert_eq!(
        (receipt.workflow.run_id, receipt.workflow.run_attempt),
        (8, 1)
    );
    assert_eq!(receipt.version.commit_count, None);
}

#[test]
fn continuation_refuses_mutable_foreign_or_noncanonical_stable_hints() {
    let (plan, body, release) = fixture();
    for (pointer, value) in [
        ("/immutable", json!(false)),
        ("/draft", json!(true)),
        ("/prerelease", json!(true)),
        ("/author/id", json!(1)),
        ("/author/login", json!("other")),
        ("/tag_name", json!("other")),
        ("/body", json!(format!("{body}extra\n"))),
    ] {
        let mut changed = release.clone();
        *changed.pointer_mut(pointer).expect("existing field") = value;
        let parsed = serde_json::from_value(changed).expect("release response");
        assert!(
            stable_receipt_hint(&parsed, &plan, 41, &body).is_err(),
            "{pointer}"
        );
    }
    let parsed = serde_json::from_value(release).expect("release response");
    assert!(stable_receipt_hint(&parsed, &plan, 42, &body).is_err());
    let mut changed = plan;
    changed.products[0].bins.push("unexpected".to_owned());
    assert!(stable_receipt_hint(&parsed, &changed, 41, &body).is_err());
}
