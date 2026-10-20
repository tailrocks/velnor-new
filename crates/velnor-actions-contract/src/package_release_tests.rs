use super::*;
use support::{archive_count, fixture, jackin_preview, jackin_receipt};

#[path = "package_release_test_support.rs"]
pub(crate) mod support;

#[test]
fn validates_preview_receipt_and_round_trips_canonical_bytes() {
    let receipt = fixture(PackageReleaseChannel::Preview);
    receipt.validate().expect("valid preview receipt");
    assert_eq!(receipt.source.event, PackageReleaseEvent::WorkflowRun);
    let bytes = receipt.canonical_bytes().expect("canonical serialization");
    assert_eq!(
        parse_package_release_receipt(&bytes).expect("canonical parse"),
        receipt
    );
    assert!(
        receipt
            .canonical_digest()
            .expect("retry digest")
            .starts_with("b3-")
    );
}

#[test]
fn preview_receipt_rejects_dispatch_and_legacy_push_producing_events() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.source.event = PackageReleaseEvent::WorkflowDispatch;
    assert!(receipt.validate().is_err());
    let mut value = serde_json::to_value(fixture(PackageReleaseChannel::Preview)).expect("receipt");
    value["source"]["event"] = serde_json::json!("push");
    assert!(serde_json::from_value::<PackageReleaseReceipt>(value).is_err());
}

#[test]
fn preserves_stable_manual_dispatch_and_package_tag_identity() {
    let receipt = fixture(PackageReleaseChannel::Stable);
    receipt.validate().expect("stable receipt remains valid");
}

#[test]
fn preview_receipt_requires_main_and_stable_receipt_keeps_custom_default_branch() {
    let mut preview = fixture(PackageReleaseChannel::Preview);
    preview.source.default_branch = "develop".to_owned();
    preview.source.source_ref = "refs/heads/develop".to_owned();
    preview.workflow.workflow_ref = preview.source.source_ref.clone();
    preview.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        preview.repository.slug, preview.workflow.path, preview.source.source_ref
    );
    assert!(preview.validate().is_err());

    let mut stable = fixture(PackageReleaseChannel::Stable);
    stable.source.default_branch = "release/1.2".to_owned();
    stable.source.source_ref = "refs/heads/release/1.2".to_owned();
    stable.workflow.workflow_ref = stable.source.source_ref.clone();
    stable.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        stable.repository.slug, stable.workflow.path, stable.source.source_ref
    );
    stable
        .validate()
        .expect("stable release retains its configured branch");

    stable.source.default_branch = "release/.hidden".to_owned();
    stable.source.source_ref = "refs/heads/release/.hidden".to_owned();
    stable.workflow.workflow_ref = stable.source.source_ref.clone();
    stable.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        stable.repository.slug, stable.workflow.path, stable.source.source_ref
    );
    assert!(stable.validate().is_err());
}

#[test]
fn stable_receipt_uses_shared_archive_inventory_and_permanent_version_tag() {
    let receipt = fixture(PackageReleaseChannel::Stable);
    receipt.validate().expect("stable archive receipt");
    assert_eq!(receipt.source.event, PackageReleaseEvent::WorkflowDispatch);
    assert_eq!(receipt.tag, "v1.2.3");
    assert_eq!(receipt.version.commit_count, None);
    assert_eq!(receipt.version.source_sha7, None);
    assert_eq!(
        receipt.products[0].payloads[0].format,
        PackageReleasePayloadFormat::TarGz
    );
    let mut raw = serde_json::to_value(receipt).expect("receipt");
    raw["products"][0]["payloads"][0]["format"] = serde_json::json!("raw-binary");
    assert!(serde_json::from_value::<PackageReleaseReceipt>(raw).is_err());
}

#[test]
fn stable_receipt_rejects_push_event() {
    let receipt = fixture(PackageReleaseChannel::Stable);
    let mut value = serde_json::to_value(receipt).expect("receipt JSON");
    value["source"]["event"] = serde_json::json!("push");
    assert!(serde_json::from_value::<PackageReleaseReceipt>(value).is_err());
}

#[test]
fn receipt_rejects_noncanonical_channel_workflow_path() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.workflow.path = ".github/workflows/release.yml".to_owned();
    receipt.workflow.signer_workflow = receipt.workflow.path.clone();
    receipt.evidence.signer_workflow_ref =
        "jackin-project/jackin/.github/workflows/release.yml@refs/heads/main".to_owned();
    assert!(receipt.validate().is_err());
}

#[test]
fn receipt_requires_the_fixed_channel_signer_job() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.workflow.signer_job_id = "another-job".to_owned();
    assert!(receipt.validate().is_err());
}

#[test]
fn receipt_producer_run_must_be_the_enclosing_workflow_attempt() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.workflow.producer_run_id += 1;
    assert!(receipt.validate().is_err());

    let mut receipt = fixture(PackageReleaseChannel::Stable);
    receipt.workflow.producer_run_attempt += 1;
    assert!(receipt.validate().is_err());
}

#[test]
fn rejects_duplicate_keys_unknown_fields_and_noncanonical_bytes() {
    let receipt = fixture(PackageReleaseChannel::Preview);
    let bytes = receipt.canonical_bytes().expect("canonical serialization");
    let text = String::from_utf8(bytes).expect("UTF-8");
    let duplicate = text.replacen("{\"assets\":", "{\"assets\":[],\"assets\":", 1);
    assert!(parse_package_release_receipt(duplicate.as_bytes()).is_err());
    assert!(parse_package_release_receipt(format!(" {text}").as_bytes()).is_err());
    assert!(
        parse_package_release_receipt(
            text.replace("\"schema\":", "\"extra\":true,\"schema\":")
                .as_bytes()
        )
        .is_err()
    );
}

#[test]
fn rejects_wrong_asset_subject_and_reserved_receipt_recursion() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.assets[0].subject_name = Some("different.tar.gz".to_owned());
    assert!(receipt.validate().is_err());
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.companions[0].name = PACKAGE_RELEASE_RECEIPT_NAME.to_owned();
    assert!(receipt.validate().is_err());
}

#[test]
fn manual_recovery_keeps_original_run_identity_in_receipt() {
    let original = fixture(PackageReleaseChannel::Preview);
    let recovered = original.clone();
    // The recovery invocation is intentionally absent from canonical identity.
    assert_eq!(
        recovered
            .canonical_bytes()
            .expect("recovered receipt bytes"),
        original.canonical_bytes().expect("original receipt bytes")
    );
    let mut rebound = recovered;
    rebound.workflow.run_id = 999;
    assert_ne!(
        rebound.canonical_digest().expect("rebound digest"),
        original.canonical_digest().expect("original digest")
    );
}

#[test]
fn validates_jackin_six_payloads_and_exact_twenty_seven_receipt_assets() {
    let preview = jackin_preview();
    let mut receipt = jackin_receipt(&preview);
    assert_eq!(archive_count(&receipt), 6);
    assert_eq!(receipt.assets.len(), 27);
    receipt
        .validate_against_config(&preview)
        .expect("exact Jackin inventory");
    receipt.assets.pop();
    assert!(
        receipt.validate_against_config(&preview).is_err(),
        "missing asset rejected"
    );
}

#[test]
fn preview_validation_rejects_malformed_receipt_even_when_inventory_matches() {
    let preview = jackin_preview();
    let mut receipt = jackin_receipt(&preview);
    receipt.assets[0].sha256 = "not-a-sha256".to_owned();
    assert!(receipt.validate_against_config(&preview).is_err());
}
