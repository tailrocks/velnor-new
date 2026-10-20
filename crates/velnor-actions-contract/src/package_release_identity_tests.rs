use super::validate_version;
use crate::package_release::{PackageReleaseChannel, tests::support::fixture};

#[test]
fn rolling_tag_keeps_full_source_derived_version_but_has_no_permanent_preview_tag() {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.tag = "preview".to_owned();
    assert!(validate_version(&receipt).is_ok());
    receipt.tag = format!("v{}", receipt.version.value);
    assert!(validate_version(&receipt).is_err());
    receipt.tag = "preview".to_owned();
    receipt.version.source_sha7 = Some("0000000".to_owned());
    assert!(validate_version(&receipt).is_err());
}

#[test]
fn stable_tag_is_permanent_application_version_not_product_or_rolling_alias() {
    let mut receipt = fixture(PackageReleaseChannel::Stable);
    receipt.tag = format!("v{}", receipt.version.base);
    assert!(validate_version(&receipt).is_ok());
    for tag in [
        "preview".to_owned(),
        format!("{}-v{}", receipt.products[0].package, receipt.version.base),
        format!("v{}+local", receipt.version.base),
    ] {
        receipt.tag = tag;
        assert!(validate_version(&receipt).is_err());
    }
}
