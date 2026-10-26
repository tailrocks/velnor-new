// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseAssetKind, PackageReleaseReceipt},
};

use super::{
    PackageReleaseSigningIdentity, VerifiedSignedPackage, absolute_env, cosign,
    default_verification_deadline, read_regular_file, receipt, sha256, validate_identity,
    validate_release_directory,
};
use crate::{OrchestratorError, internal::internal};

pub(crate) fn verify_signed_package(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<VerifiedSignedPackage, OrchestratorError> {
    verify_signed_package_with_deadline(directory, plan, expected, default_verification_deadline()?)
}

pub(crate) fn verify_signed_package_with_deadline(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VerifiedSignedPackage, OrchestratorError> {
    validate_identity(plan, expected)?;
    let receipt_path = directory.join("release.json");
    let bytes = read_regular_file(&receipt_path, 4 * 1024 * 1024)?;
    let value = velnor_actions_contract::parse_strict_json(
        std::str::from_utf8(&bytes).map_err(|_| internal("package_receipt_not_utf8"))?,
    )
    .map_err(|_| internal("package_receipt_invalid_json"))?;
    let receipt: PackageReleaseReceipt =
        serde_json::from_value(value).map_err(|_| internal("package_receipt_invalid_schema"))?;
    receipt
        .validate()
        .map_err(|_| internal("package_receipt_invalid"))?;
    if receipt
        .canonical_bytes()
        .map_err(|_| internal("package_receipt_canonical_encode"))?
        != bytes
    {
        return Err(internal("package_receipt_noncanonical"));
    }
    receipt::require_expected_identity(&receipt, plan, expected)?;
    validate_release_directory(directory, &receipt)?;
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let cosign = cosign::acquire_cosign(&runner_temp)?;
    for asset in &receipt.assets {
        let path = directory.join(&asset.name);
        let data = read_regular_file(&path, 1024 * 1024 * 1024)?;
        if data.len() as u64 != asset.size || sha256(&data) != asset.sha256 {
            return Err(internal("package_asset_digest_mismatch"));
        }
        if matches!(
            asset.kind,
            PackageReleaseAssetKind::SignatureBundle
                | PackageReleaseAssetKind::CompanionSignatureBundle
        ) {
            continue;
        }
        if matches!(
            asset.kind,
            PackageReleaseAssetKind::Payload | PackageReleaseAssetKind::Companion
        ) {
            let bundle_path = directory.join(format!("{}.bundle", asset.name));
            cosign::verify_blob(&cosign, &path, &bundle_path, expected, deadline)?;
        }
    }
    let bundle_path = directory.join("release.json.bundle");
    cosign::verify_blob(&cosign, &receipt_path, &bundle_path, expected, deadline)?;
    Ok(VerifiedSignedPackage {
        plan: plan.clone(),
        receipt,
        directory: directory.to_path_buf(),
    })
}
