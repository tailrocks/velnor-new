// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::{
    MAX_PACKAGE_RELEASE_ASSETS, PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME, PACKAGE_RELEASE_RECEIPT_NAME,
    PackageReleaseAsset, PackageReleaseAssetKind, PackageReleaseReceipt,
};
use crate::ContractError;

use super::{invalid, valid_asset_name};

type Binding = (
    PackageReleaseAssetKind,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(super) fn validate_assets(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let expected = expected_bindings(receipt)?;
    if receipt.assets.len() != expected.len() || receipt.assets.len() > MAX_PACKAGE_RELEASE_ASSETS {
        return Err(invalid("asset_inventory_size_mismatch"));
    }
    validate_ordered_assets(&receipt.assets, &expected)
}

fn expected_bindings(
    receipt: &PackageReleaseReceipt,
) -> Result<BTreeMap<String, Binding>, ContractError> {
    let mut expected = BTreeMap::<String, Binding>::new();
    for product in &receipt.products {
        for archive in &product.payloads {
            insert_archive_bindings(&mut expected, &product.id, &archive.name, &archive.target)?;
        }
    }
    insert_companion_bindings(&mut expected, receipt)?;
    insert(
        &mut expected,
        "SHA256SUMS",
        PackageReleaseAssetKind::ChecksumList,
        None,
        None,
        None,
    )?;
    Ok(expected)
}

fn insert_archive_bindings(
    expected: &mut BTreeMap<String, Binding>,
    product_id: &str,
    archive: &str,
    target: &str,
) -> Result<(), ContractError> {
    let owner = Some(product_id.to_owned());
    let target = Some(target.to_owned());
    insert(
        expected,
        archive,
        PackageReleaseAssetKind::Payload,
        owner.clone(),
        target.clone(),
        Some(archive.to_owned()),
    )?;
    insert(
        expected,
        &format!("{archive}.sha256"),
        PackageReleaseAssetKind::Sha256,
        owner.clone(),
        target.clone(),
        Some(archive.to_owned()),
    )?;
    insert(
        expected,
        &format!("{archive}.bundle"),
        PackageReleaseAssetKind::SignatureBundle,
        owner.clone(),
        target.clone(),
        Some(archive.to_owned()),
    )?;
    insert(
        expected,
        &format!("{archive}.sbom.json"),
        PackageReleaseAssetKind::Sbom,
        owner,
        target,
        Some(archive.to_owned()),
    )
}

fn insert_companion_bindings(
    expected: &mut BTreeMap<String, Binding>,
    receipt: &PackageReleaseReceipt,
) -> Result<(), ContractError> {
    for companion in &receipt.companions {
        insert(
            expected,
            &companion.name,
            PackageReleaseAssetKind::Companion,
            None,
            None,
            Some(companion.name.clone()),
        )?;
        insert(
            expected,
            &format!("{}.bundle", companion.name),
            PackageReleaseAssetKind::CompanionSignatureBundle,
            None,
            None,
            Some(companion.name.clone()),
        )?;
    }
    if expected.contains_key(PACKAGE_RELEASE_RECEIPT_NAME)
        || expected.contains_key(PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME)
    {
        return Err(invalid("reserved_receipt_asset_name"));
    }
    Ok(())
}

fn validate_ordered_assets(
    assets: &[PackageReleaseAsset],
    expected: &BTreeMap<String, Binding>,
) -> Result<(), ContractError> {
    let mut previous: Option<&str> = None;
    for asset in assets {
        if previous.is_some_and(|name| name >= asset.name.as_str()) {
            return Err(invalid("assets_not_sorted_unique"));
        }
        previous = Some(&asset.name);
        validate_asset(asset, expected)?;
    }
    Ok(())
}

fn insert(
    map: &mut BTreeMap<String, Binding>,
    name: &str,
    kind: PackageReleaseAssetKind,
    product_id: Option<String>,
    target: Option<String>,
    subject: Option<String>,
) -> Result<(), ContractError> {
    if !valid_asset_name(name)
        || name == PACKAGE_RELEASE_RECEIPT_NAME
        || name == PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME
        || map
            .insert(name.to_owned(), (kind, product_id, target, subject))
            .is_some()
    {
        return Err(invalid("duplicate_or_reserved_asset_name"));
    }
    Ok(())
}

fn validate_asset(
    asset: &PackageReleaseAsset,
    expected: &BTreeMap<String, Binding>,
) -> Result<(), ContractError> {
    if asset.size == 0 || asset.sha256.len() != 64 || !lower_hex(&asset.sha256) {
        return Err(invalid("bad_asset_byte_identity"));
    }
    let Some((kind, product_id, target, subject)) = expected.get(&asset.name) else {
        return Err(invalid("unexpected_asset"));
    };
    if &asset.kind != kind
        || &asset.product_id != product_id
        || &asset.target != target
        || &asset.subject_name != subject
    {
        return Err(invalid("asset_subject_binding_mismatch"));
    }
    Ok(())
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
