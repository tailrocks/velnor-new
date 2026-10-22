use super::validate_version;
use crate::package_release::{PackageReleaseChannel, tests::support::fixture};

#[test]
fn preview_tag_is_per_source_and_keeps_full_source_derived_version() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.tag = format!("preview-{}", receipt.source.commit_sha);
    assert!(validate_version(&receipt).is_ok());
    receipt.tag = format!("v{}", receipt.version.value);
    assert!(validate_version(&receipt).is_err());
    receipt.tag = format!("preview-{}", receipt.source.commit_sha);
    receipt.tag.push('a');
    assert!(validate_version(&receipt).is_err());
    receipt.tag = format!("preview-{}", receipt.source.commit_sha);
    receipt.version.source_sha7 = Some("0000000".to_owned());
    assert!(validate_version(&receipt).is_err());
}

#[test]
fn stable_tag_is_permanent_application_version_not_product_or_rolling_alias() {
    let mut receipt = fixture(PackageReleaseChannel::Stable);
    receipt.tag = format!("v{}", receipt.version.base);
    assert!(validate_version(&receipt).is_ok());
    for tag in [
        format!("preview-{}", receipt.source.commit_sha),
        format!("{}-v{}", receipt.products[0].package, receipt.version.base),
        format!("v{}+local", receipt.version.base),
    ] {
        receipt.tag = tag;
        assert!(validate_version(&receipt).is_err());
    }
}

#[test]
fn preview_workflow_authority_can_advance_without_retargeting_source_version_or_tag() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    let source_sha = receipt.source.commit_sha.clone();
    let source_version = receipt.version.value.clone();
    receipt.workflow.authority_sha = "b".repeat(40);

    receipt
        .validate()
        .expect("authenticated authority is independent of source syntax");
    assert_eq!(receipt.source.commit_sha, source_sha);
    assert_eq!(receipt.version.value, source_version);
    assert_eq!(receipt.tag, format!("preview-{source_sha}"));

    let mut malformed = receipt.clone();
    malformed.workflow.authority_sha = "B".repeat(40);
    assert!(malformed.validate().is_err());
    malformed.workflow.authority_sha = "b".repeat(39);
    assert!(malformed.validate().is_err());
    malformed.workflow.authority_sha = "g".repeat(40);
    assert!(malformed.validate().is_err());

    let mut wrong_tag = receipt;
    wrong_tag.tag = format!("preview-{}", wrong_tag.workflow.authority_sha);
    assert!(wrong_tag.validate().is_err());
}

#[test]
fn stable_receipt_still_requires_workflow_authority_to_match_source() {
    let mut receipt = fixture(PackageReleaseChannel::Stable);
    receipt.validate().expect("stable identity");
    receipt.workflow.authority_sha = "b".repeat(40);
    assert!(receipt.validate().is_err());
}
