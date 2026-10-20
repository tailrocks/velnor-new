//! Authenticated retained generations used only to admit rolling predecessors.
//!
//! The reconstructed plan supplies receipt inventory to the existing verifier;
//! it cannot authorize installation or a new build. Every byte still requires
//! the original workflow's signatures, attestation, source CI and signer job.

use super::super::{
    artifact_zip,
    github::{GitHubApi, sha256},
};
use super::{archive, original_signing_identity, runner_root, validate_artifact};
use crate::{
    OrchestratorError,
    internal::internal,
    package_release_signer::{VerifiedAttestedSignedPackage, verify_attested_signed_package},
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    config::RustBinaryReleaseCompanion,
    package_release::{
        PackageReleaseChannel, PackageReleaseReceipt, parse_package_release_receipt,
    },
    publication_retry::rolling::RollingOriginal,
};

pub(in crate::package_release_publish) struct RollingOriginalEvidence {
    pub(in crate::package_release_publish) signed: VerifiedAttestedSignedPackage,
    _directory: tempfile::TempDir,
}

pub(in crate::package_release_publish) fn verify(
    api: &GitHubApi,
    repository: &str,
    repository_id: u64,
    original: &RollingOriginal,
) -> Result<RollingOriginalEvidence, OrchestratorError> {
    let artifacts = api.workflow_artifacts(original.run_id)?;
    let name = format!(
        "velnor-preview-signed-{}-{}",
        original.run_id, original.run_attempt
    );
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == name)
        .collect();
    if matches.len() != 1
        || matches[0].id != original.artifact_id
        || matches[0].digest.as_deref()
            != Some(format!("sha256:{}", original.archive_sha256).as_str())
    {
        return Err(internal("rolling_original_artifact_identity"));
    }
    let source_sha = validate_artifact(matches[0], original.run_id, repository_id, "main")?;
    let bytes = api.download_artifact_archive(original.artifact_id)?;
    if bytes.len() as u64 != matches[0].size_in_bytes || sha256(&bytes) != original.archive_sha256 {
        return Err(internal("rolling_original_artifact_digest"));
    }
    let receipt_bytes = artifact_zip::read_manifest_in_scope(&bytes, "release.json", 1024 * 1024)?;
    let receipt = parse_package_release_receipt(&receipt_bytes)
        .map_err(|_| internal("rolling_original_receipt"))?;
    if receipt.channel != PackageReleaseChannel::Preview
        || receipt.repository.slug != repository
        || receipt.repository.id != repository_id
        || receipt.workflow.run_id != original.run_id
        || receipt.workflow.run_attempt != original.run_attempt
        || receipt.source.commit_sha != source_sha
    {
        return Err(internal("rolling_original_receipt_identity"));
    }
    let plan = inventory_plan(&receipt)?;
    let directory = tempfile::tempdir_in(runner_root()?)
        .map_err(|_| internal("rolling_original_temporary_directory"))?;
    archive::extract_signed_assets(&bytes, directory.path(), &plan)?;
    let identity = original_signing_identity(
        api,
        &plan,
        &receipt,
        original.run_id,
        original.run_attempt,
        repository_id,
    )?;
    let signed = verify_attested_signed_package(directory.path(), &plan, &identity)?;
    Ok(RollingOriginalEvidence {
        signed,
        _directory: directory,
    })
}

fn inventory_plan(
    receipt: &PackageReleaseReceipt,
) -> Result<PackageReleasePlan, OrchestratorError> {
    let plan =
        PackageReleasePlan {
            schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
            channel: receipt.channel,
            repository: receipt.repository.slug.clone(),
            default_branch: receipt.source.default_branch.clone(),
            base_version: receipt.version.base.clone(),
            products:
                receipt
                    .products
                    .iter()
                    .map(|product| {
                        PackageReleasePlanProduct {
            id: product.id.clone(), package: product.package.clone(), bins: product.bins.clone(),
            targets: product.payloads.iter().map(|payload| payload.target.clone()).collect(),
            format: velnor_actions_contract::package_release::PackageReleasePayloadFormat::TarGz,
        }
                    })
                    .collect(),
            companions: receipt
                .companions
                .iter()
                .map(|companion| RustBinaryReleaseCompanion {
                    id: companion.id.clone(),
                    path: companion.name.clone(),
                    asset_name: companion.name.clone(),
                })
                .collect(),
        };
    plan.validate()
        .map_err(|_| internal("rolling_original_inventory"))?;
    Ok(plan)
}
