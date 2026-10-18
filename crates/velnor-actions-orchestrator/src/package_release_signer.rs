//! Credentialed signer for a plan-bound native package release.
//!
//! This module consumes only the static plan and retained producer bytes. It
//! never loads repository configuration, resolves task code, or executes source.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseAssetKind, PackageReleaseEvent, PackageReleaseReceipt},
    parse_package_release_plan,
};

use crate::{
    OrchestratorError,
    internal::internal,
    package_release_artifacts::{
        PackageOutputIdentity, VerifiedPackageArtifact, verify_package_output,
    },
};

mod runtime;
use runtime::{
    absolute_env, preview_companions, preview_products, read_regular_file, release_version,
    require_private_directory, sha256, signing_identity_from_env, validate_identity,
    validate_release_directory, write_new_file,
};

#[path = "package_release_signer_attestation.rs"]
mod attestation;
#[path = "package_release_signer_cosign.rs"]
mod cosign;
#[path = "package_release_signer_receipt.rs"]
mod receipt;
#[cfg(test)]
#[path = "package_release_signer_tests.rs"]
mod tests;

/// Private CLI operation for signing a statically planned package release.
pub const SIGN_PACKAGE_RELEASE_OP: &str = "sign-package-release-v1";
const PLAN_FILE: &str = "package-release-plan.json";
const INPUT_DIR: &str = "preview-input";
const OUTPUT_DIR: &str = "preview-signed";
const PUBLISH_INPUT_DIR: &str = "preview-publish-input";
const HANDOFF_FILE: &str = "package-output.json";

/// Trusted GitHub workflow identity, independently captured from runner facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageReleaseSigningIdentity {
    pub repository: String,
    pub repository_id: u64,
    pub source_sha: String,
    pub source_ref: String,
    pub default_branch: String,
    pub event: PackageReleaseEvent,
    pub workflow_path: String,
    pub workflow_ref: String,
    pub workflow_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub ci_run_id: u64,
    pub ci_run_attempt: u32,
    pub commit_count: Option<u64>,
}

/// Verified receipt plus its staged immutable assets; fields are private so
/// publication cannot accept a caller-constructed proof.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedSignedPackage {
    plan: PackageReleasePlan,
    receipt: PackageReleaseReceipt,
    directory: PathBuf,
}

/// Signed package whose source and workflow provenance has also been checked
/// against GitHub's cryptographically verified artifact attestation.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedAttestedSignedPackage {
    signed: VerifiedSignedPackage,
}

impl VerifiedAttestedSignedPackage {
    pub(crate) fn plan(&self) -> &PackageReleasePlan {
        self.signed.plan()
    }

    pub(crate) fn receipt(&self) -> &PackageReleaseReceipt {
        self.signed.receipt()
    }

    pub(crate) fn directory(&self) -> &Path {
        self.signed.directory()
    }
}

impl VerifiedSignedPackage {
    pub(crate) fn plan(&self) -> &PackageReleasePlan {
        &self.plan
    }
    pub(crate) fn receipt(&self) -> &PackageReleaseReceipt {
        &self.receipt
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Execute the private signer operation using fixed runner paths and a strict
/// static plan request. No path or signer identity comes from producer data.
///
/// # Errors
/// Returns a contract error if the request, identity, or signed artifact is invalid.
pub fn sign_package_release_from_env() -> Result<(), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let root = runner_temp.join("velnor");
    require_private_directory(&root)?;
    let request = absolute_env("VELNOR_REQUEST_FILE")?;
    if request != root.join(PLAN_FILE) {
        return Err(internal("package_signer_request_path_mismatch"));
    }
    let plan_bytes = read_regular_file(&request, 1024 * 1024)?;
    let plan = parse_package_release_plan(&plan_bytes)
        .map_err(|_| internal("package_signer_plan_invalid"))?;
    let input = root.join(INPUT_DIR);
    let output = root.join(OUTPUT_DIR);
    require_private_directory(&input)?;
    let identity = signing_identity_from_env(&plan)?;
    sign_package_release(&plan, &identity, &input, &output)?;
    Ok(())
}

/// Verify retained signer bytes and GitHub's exact run attestation before publication.
pub(crate) fn verify_attested_signed_package_from_env()
-> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    let (plan, expected, input) = publisher_context_from_env()?;
    verify_attested_signed_package(&input, &plan, &expected)
}

/// Verify signed assets and the GitHub attestation using an authenticated
/// original workflow identity, including when executing a recovery run.
pub(crate) fn verify_attested_signed_package(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    let mut signed = verify_signed_package(directory, plan, expected)?;
    attestation::verify_release_attestation(&signed, expected)?;
    signed.plan = plan.clone();
    Ok(VerifiedAttestedSignedPackage { signed })
}

fn publisher_context_from_env()
-> Result<(PackageReleasePlan, PackageReleaseSigningIdentity, PathBuf), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let root = runner_temp.join("velnor");
    let request = absolute_env("VELNOR_REQUEST_FILE")?;
    if request != root.join(PLAN_FILE) {
        return Err(internal("package_signer_request_path_mismatch"));
    }
    let bytes = read_regular_file(&request, 1024 * 1024)?;
    let plan =
        parse_package_release_plan(&bytes).map_err(|_| internal("package_signer_plan_invalid"))?;
    let input = root.join(PUBLISH_INPUT_DIR);
    require_private_directory(&input)?;
    let expected = signing_identity_from_env(&plan)?;
    Ok((plan, expected, input))
}

/// Independently re-admit producer bytes, create sidecars, sign payloads and
/// companions, then sign the canonical receipt last and verify it immediately.
pub(crate) fn sign_package_release(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    input_dir: &Path,
    output_dir: &Path,
) -> Result<VerifiedSignedPackage, OrchestratorError> {
    plan.validate()
        .map_err(|_| internal("package_signer_plan_invalid"))?;
    validate_identity(plan, identity)?;
    let (version, tag, commit_count, source_sha7) = release_version(plan, identity)?;
    let products = preview_products(plan);
    let companions = preview_companions(plan);
    let verified = verify_package_output(
        input_dir,
        HANDOFF_FILE,
        output_dir,
        &products,
        &companions,
        PackageOutputIdentity {
            source_commit: &identity.source_sha,
            source_ref: &identity.source_ref,
            version: &version,
        },
    )?;
    let mut artifacts = verified.artifacts.clone();
    artifacts.extend(verified.write_checksum_assets()?);

    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let cosign = cosign::acquire_cosign(&runner_temp)?;
    let mut signatures = Vec::new();
    let signed_subjects = artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.release_asset.kind,
                PackageReleaseAssetKind::Payload | PackageReleaseAssetKind::Companion
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for artifact in &signed_subjects {
        signatures.push(cosign::sign_and_verify_blob(&cosign, artifact, identity)?);
    }
    artifacts.extend(signatures);
    artifacts.sort_by(|left, right| left.name.cmp(&right.name));

    let receipt = receipt::make_receipt(
        plan,
        identity,
        &version,
        &tag,
        commit_count,
        source_sha7,
        artifacts,
    );
    receipt
        .validate()
        .map_err(|_| internal("package_signer_receipt_invalid"))?;
    let receipt_path = output_dir.join("release.json");
    let receipt_bytes = receipt
        .canonical_bytes()
        .map_err(|_| internal("package_signer_receipt_encode"))?;
    write_new_file(&receipt_path, &receipt_bytes)?;
    let receipt_artifact = VerifiedPackageArtifact::receipt_subject(&receipt_path, &receipt_bytes);
    let bundle = cosign::sign_and_verify_blob(&cosign, &receipt_artifact, identity)?;
    if bundle.name != "release.json.bundle" {
        return Err(internal("package_signer_receipt_bundle_name"));
    }
    verify_signed_package(output_dir, plan, identity)?;
    Ok(VerifiedSignedPackage {
        plan: plan.clone(),
        receipt,
        directory: output_dir.to_path_buf(),
    })
}

/// Authenticate the canonical receipt and every signed asset against trusted
/// plan and workflow facts. Used again by the publisher after artifact download.
pub(crate) fn verify_signed_package(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
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
            cosign::verify_blob(&cosign, &path, &bundle_path, expected)?;
        }
    }
    let bundle_path = directory.join("release.json.bundle");
    cosign::verify_blob(&cosign, &receipt_path, &bundle_path, expected)?;
    Ok(VerifiedSignedPackage {
        plan: plan.clone(),
        receipt,
        directory: directory.to_path_buf(),
    })
}
