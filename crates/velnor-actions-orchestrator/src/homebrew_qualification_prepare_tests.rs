use super::super::{FORMULA_SPEC, PLAN};
use super::selected_payload_hashes;
use velnor_actions_contract::{
    homebrew_spec::{HomebrewFormulaSpec, validate_homebrew_projection},
    package_release::{PackageReleaseAsset, PackageReleaseAssetKind},
    parse_package_release_plan, parse_strict_json,
};

fn projection() -> velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection {
    let plan = parse_package_release_plan(PLAN).expect("pinned plan fixture");
    let value = parse_strict_json(std::str::from_utf8(FORMULA_SPEC).expect("UTF-8 spec"))
        .expect("strict spec fixture");
    let spec: HomebrewFormulaSpec = serde_json::from_value(value).expect("typed spec fixture");
    validate_homebrew_projection(&spec, &plan).expect("valid formula projection")
}

fn asset(name: &str, sha256: &str) -> PackageReleaseAsset {
    PackageReleaseAsset {
        name: name.to_owned(),
        kind: PackageReleaseAssetKind::Payload,
        product_id: None,
        target: None,
        subject_name: None,
        size: 1,
        sha256: sha256.to_owned(),
    }
}

fn selected_assets(
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
) -> Vec<PackageReleaseAsset> {
    projection
        .selected_payloads()
        .iter()
        .enumerate()
        .map(|(index, payload)| asset(payload.asset_name(), &format!("{:064x}", index + 1)))
        .collect()
}

#[test]
fn formula_hashes_include_selected_payloads_and_ignore_other_receipt_assets() {
    let projection = projection();
    let mut assets = selected_assets(&projection);
    assets.push(PackageReleaseAsset {
        name: "release.json".to_owned(),
        kind: PackageReleaseAssetKind::ChecksumList,
        product_id: None,
        target: None,
        subject_name: None,
        size: 1,
        sha256: "f".repeat(64),
    });

    let hashes = selected_payload_hashes(&projection, &assets).expect("selected hashes");
    assert_eq!(hashes.len(), projection.selected_payloads().len());
    for payload in projection.selected_payloads() {
        assert!(hashes.contains_key(payload.asset_name()));
    }
    assert!(!hashes.contains_key("release.json"));
}

#[test]
fn formula_hashes_reject_missing_selected_asset() {
    let projection = projection();
    let mut assets = selected_assets(&projection);
    assets.pop();

    assert!(selected_payload_hashes(&projection, &assets).is_err());
}

#[test]
fn formula_hashes_reject_duplicate_selected_asset() {
    let projection = projection();
    let mut assets = selected_assets(&projection);
    let duplicate = assets[0].clone();
    assets.push(duplicate);

    assert!(selected_payload_hashes(&projection, &assets).is_err());
}
