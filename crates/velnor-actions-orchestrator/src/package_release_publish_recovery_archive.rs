//! Receipt-specific admission over the shared closed ZIP mechanics.
use super::super::{
    artifact_zip::{self, ExpectedZipEntry},
    github::sha256,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::path::Path;
use velnor_actions_contract::{
    PackageReleasePlan, package_release::PackageReleaseReceipt, strict_json::parse_strict_json,
};

const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;
const MAX_RECEIPT_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub(super) fn extract_signed_assets(
    archive_bytes: &[u8],
    destination: &Path,
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    let receipt_bytes =
        artifact_zip::read_manifest_in_scope(archive_bytes, "release.json", MAX_RECEIPT_BYTES)?;
    let receipt = parse_receipt(&receipt_bytes, plan)?;
    let mut expected = receipt
        .assets
        .iter()
        .map(|asset| ExpectedZipEntry {
            name: asset.name.clone(),
            size: Some(asset.size),
            sha256: Some(asset.sha256.clone()),
            max_bytes: MAX_UNPACKED_BYTES,
            unix_permissions: None,
        })
        .collect::<Vec<_>>();
    expected.push(ExpectedZipEntry {
        name: "release.json".into(),
        size: Some(receipt_bytes.len() as u64),
        sha256: Some(sha256(&receipt_bytes)),
        max_bytes: MAX_RECEIPT_BYTES,
        unix_permissions: None,
    });
    // Historical signature bytes are authenticated by the subsequent crypto
    // verifier, not by an invented self-referential receipt-bundle hash.
    expected.push(ExpectedZipEntry {
        name: "release.json.bundle".into(),
        size: None,
        sha256: None,
        max_bytes: MAX_RECEIPT_BUNDLE_BYTES,
        unix_permissions: None,
    });
    artifact_zip::extract_in_scope(archive_bytes, destination, &expected)?;
    Ok(receipt)
}

fn parse_receipt(
    bytes: &[u8],
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    let value: Value = parse_strict_json(
        std::str::from_utf8(bytes).map_err(|_| internal("package_recovery_receipt_utf8"))?,
    )
    .map_err(|_| internal("package_recovery_receipt_json"))?;
    let receipt: PackageReleaseReceipt =
        serde_json::from_value(value).map_err(|_| internal("package_recovery_receipt_shape"))?;
    receipt
        .validate_against_plan(plan)
        .map_err(|_| internal("package_recovery_receipt_plan_mismatch"))?;
    if receipt
        .canonical_bytes()
        .map_err(|_| internal("package_recovery_receipt_encode"))?
        != bytes
    {
        return Err(internal("package_recovery_receipt_not_canonical"));
    }
    Ok(receipt)
}

#[cfg(test)]
#[path = "artifact_zip_recovery_characterization_tests.rs"]
mod characterization_tests;
