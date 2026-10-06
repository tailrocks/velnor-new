//! Private fixtures test bindings; no actual Foundation row is admitted.

use super::*;

#[test]
fn absent_real_supplier_receipts_fail_closed() {
    assert!(RootLinuxColdFoundation::require().is_err());
}

#[test]
fn foundation_identity_binds_each_receipt_and_recipe_field() {
    let fixture = RootLinuxColdFoundation {
        source_receipt_sha256: "fixture-source-receipt",
        artifact_receipt_sha256: "fixture-artifact-receipt",
        behavior_receipt_sha256: "fixture-behavior-receipt",
        installer_identity: "fixture-installer-identity",
        mise_qualification_sha256: "fixture-mise-qualification",
    };
    assert_eq!(fixture.schema(), 1);
    assert_eq!(fixture.purpose(), "source-intent-cold-sdk-foundation");
    assert_eq!(fixture.host(), "x86_64-unknown-linux-gnu");
    assert_eq!(fixture.runtime_abi(), "source-original-fs-v3");
    let mutations: [fn(&mut RootLinuxColdFoundation); 5] = [
        |value| value.source_receipt_sha256 = "changed-source-receipt",
        |value| value.artifact_receipt_sha256 = "changed-artifact-receipt",
        |value| value.behavior_receipt_sha256 = "changed-behavior-receipt",
        |value| value.installer_identity = "changed-installer",
        |value| value.mise_qualification_sha256 = "changed-mise-qualification",
    ];
    for mutate in mutations {
        let mut changed = fixture;
        mutate(&mut changed);
        assert_ne!(
            changed.qualification_sha256(),
            fixture.qualification_sha256()
        );
    }
}
