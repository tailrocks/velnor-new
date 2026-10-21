//! Credentialed signer for a plan-bound native package release.
//!
//! This module consumes an admitted static policy/plan and retained producer bytes. It
//! never loads repository configuration, resolves task code, or executes source.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseAssetKind, PackageReleaseEvent, PackageReleaseReceipt},
};

use crate::{
    OrchestratorError,
    internal::internal,
    package_release_artifacts::{
        PackageOutputIdentity, VerifiedPackageArtifact, verify_package_output,
    },
};

#[path = "package_release_signer_request.rs"]
mod request;
mod runtime;
use runtime::{
    absolute_env, read_regular_file, release_companions, release_version,
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
const INPUT_DIR: &str = "release-output";
const OUTPUT_DIR: &str = "release-signed";
const PUBLISH_INPUT_DIR: &str = "release-publish-input";
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
    closure: std::sync::Arc<crate::filesystem_owner::held_directory::HeldDirectory>,
    identity: PackageReleaseSigningIdentity,
}

impl VerifiedAttestedSignedPackage {
    pub(crate) fn closure(
        &self,
    ) -> std::sync::Arc<crate::filesystem_owner::held_directory::HeldDirectory> {
        std::sync::Arc::clone(&self.closure)
    }

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
/// static policy request. No path or signer identity comes from producer data.
///
/// # Errors
/// Returns a contract error if the request, identity, or signed artifact is invalid.
pub fn sign_package_release_from_env() -> Result<(), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let root = runner_temp.join("velnor");
    require_private_directory(&root)?;
    let plan = request::admitted_plan(&root)?;
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
    match request::verification_route()? {
        request::VerificationRoute::Retained => {
            crate::package_release_publish::verify_original_for_promotion()
        }
        request::VerificationRoute::DirectTap => {
            let root = absolute_env("RUNNER_TEMP")?.join("velnor");
            let plan = request::direct_tap_plan(&root)?;
            let expected = runtime::published_identity_from_env(&plan)?;
            let input = root.join(PUBLISH_INPUT_DIR);
            require_private_directory(&input)?;
            verify_attested_signed_package(&input, &plan, &expected)
        }
        request::VerificationRoute::Admitted => {
            let (plan, expected, input) = publisher_context_from_env(None)?;
            verify_attested_signed_package(&input, &plan, &expected)
        }
    }
}

/// Verify signed assets and the GitHub attestation using an authenticated
/// original workflow identity, including when executing a recovery run.
pub(crate) fn verify_attested_signed_package(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(
        u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
    ))
    .map_err(|_| internal("package_verification_deadline"))?;
    let (signed, closure) =
        crate::filesystem_owner::held_directory::HeldDirectory::admit(directory, deadline, || {
            let mut signed = verify_signed_package(directory, plan, expected)?;
            attestation::verify_release_attestation(&signed, expected)?;
            signed.plan = plan.clone();
            Ok(signed)
        })?;
    Ok(VerifiedAttestedSignedPackage {
        signed,
        closure: std::sync::Arc::new(closure),
        identity: expected.clone(),
    })
}

pub(crate) fn require_retained_identity(
    signed: &VerifiedAttestedSignedPackage,
    expected: &PackageReleaseSigningIdentity,
) -> Result<(), OrchestratorError> {
    if expected != &signed.identity {
        return Err(internal("package_publish_original_changed"));
    }
    receipt::require_expected_identity(signed.receipt(), signed.plan(), expected)
}

pub(crate) fn recheck_publisher_context(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let (plan, expected, directory) =
        publisher_context_from_env(Some(signed.identity.repository_id))?;
    if &plan != signed.plan() || directory != signed.directory() || expected != signed.identity {
        return Err(internal("package_publish_original_changed"));
    }
    receipt::require_expected_identity(signed.receipt(), &plan, &expected)
}

fn publisher_context_from_env(
    repository_id: Option<u64>,
) -> Result<(PackageReleasePlan, PackageReleaseSigningIdentity, PathBuf), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let root = runner_temp.join("velnor");
    let plan = request::admitted_plan(&root)?;
    let input = root.join(PUBLISH_INPUT_DIR);
    require_private_directory(&input)?;
    let expected = match repository_id {
        Some(id) => runtime::signing_identity_for_repository(&plan, id, std::time::Instant::now())?,
        None => signing_identity_from_env(&plan)?,
    };
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
    let companions = release_companions(plan);
    let verified = verify_package_output(
        input_dir,
        HANDOFF_FILE,
        output_dir,
        &plan.products,
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
