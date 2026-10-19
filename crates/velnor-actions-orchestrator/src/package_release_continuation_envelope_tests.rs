use super::*;
use serde_json::{Value, json};
fn fixture() -> (PackageReleasePlan, PreviewIdentity, String, Value) {
    let fixture = crate::tap_test_support::fixture();
    let receipt = fixture.release.receipt;
    let bytes = receipt.canonical_bytes().expect("canonical receipt");
    let identity =
        PreviewIdentity::derive(&fixture.plan.base_version, &receipt.source.commit_sha, 9)
            .expect("preview identity");
    let body = make_recovery_body(&bytes, b"untrusted-signature-hint", &sha256(&bytes))
        .expect("canonical discovery envelope");
    let release = json!({"id":12,"tag_name":receipt.tag,"draft":false,"prerelease":true,
        "immutable":true,"body":body,"author":{"id":41_898_282,"login":"github-actions[bot]"},"assets":[]});
    (fixture.plan, identity, body, release)
}
#[test]
fn continuation_canonical_envelope_yields_only_original_run_hints() {
    let (plan, identity, body, release) = fixture();
    let parsed = serde_json::from_value(release).expect("release response");
    let receipt = receipt_hint(&parsed, &plan, &identity, 41, &body).expect("discovery hint");
    assert_eq!(
        (receipt.workflow.run_id, receipt.workflow.run_attempt),
        (8, 1)
    );
    // This is deliberately not a signature test: the next job authenticates retained bytes.
}
#[test]
fn continuation_refuses_mutable_foreign_or_noncanonical_publication_hints() {
    let (plan, identity, body, release) = fixture();
    for (pointer, value) in [
        ("/immutable", json!(false)),
        ("/draft", json!(true)),
        ("/prerelease", Value::Null),
        ("/author/id", json!(1)),
        ("/author/login", json!("other")),
        ("/tag_name", json!("other")),
        ("/body", json!(format!("{body}extra\n"))),
    ] {
        let mut changed = release.clone();
        *changed.pointer_mut(pointer).expect("existing field") = value;
        let parsed = serde_json::from_value(changed).expect("release response");
        assert!(
            receipt_hint(&parsed, &plan, &identity, 41, &body).is_err(),
            "{pointer}"
        );
    }
    let parsed = serde_json::from_value(release).expect("release response");
    assert!(receipt_hint(&parsed, &plan, &identity, 42, &body).is_err());
    let mut changed = plan;
    changed.products[0].bins.push("unexpected".to_owned());
    assert!(receipt_hint(&parsed, &changed, &identity, 41, &body).is_err());
}
