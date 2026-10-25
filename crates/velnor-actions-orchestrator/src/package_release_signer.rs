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
#[path = "package_release_signer_subjects.rs"]
mod subjects;
use runtime::{
    absolute_env, read_regular_file, release_companions, release_version,
    require_private_directory, sha256, signing_identity_from_env, validate_identity,
    validate_release_directory, write_new_file,
};
use subjects::append_attestation_subject_paths;

#[path = "package_release_signer_attestation.rs"]
mod attestation;
#[path = "package_release_signer_cosign.rs"]
mod cosign;
#[path = "package_release_signer_publication.rs"]
mod publication;

#[path = "package_release_signer_receipt.rs"]
mod receipt;
pub(crate) use attestation::{
    SdkManifestAttestationExpectation, verify_published_manifest_attestation,
};

pub(crate) use publication::verify_attested_signed_package_with_owned_tempdir;
pub(crate) use publication::verify_attested_signed_package_with_owned_tempdir_and_deadlines;

pub const SIGN_PACKAGE_RELEASE_OP: &str = "sign-package-release-v1";
pub(crate) const SIGNER_INPUT_DIR: &str = "release-output";
const OUTPUT_DIR: &str = "release-signed";
const PUBLISH_INPUT_DIR: &str = "release-publish-input";
const HANDOFF_FILE: &str = "package-output.json";

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

#[derive(Debug, Clone)]
pub(crate) struct VerifiedSignedPackage {
    plan: PackageReleasePlan,
    receipt: PackageReleaseReceipt,
    directory: PathBuf,
}

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

pub fn sign_package_release_from_env() -> Result<(), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    let root = runner_temp.join("velnor");
    require_private_directory(&root)?;
    let plan = request::admitted_plan(&root)?;
    let input = root.join(SIGNER_INPUT_DIR);
    require_private_directory(&input)?;
    let identity = signing_identity_from_env(&plan)?;
    sign_package_release(&plan, &identity, &root)?;
    Ok(())
}

pub(crate) fn verify_attested_signed_package_from_env()
-> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    let deadline = default_verification_deadline()?;
    verify_attested_signed_package_from_env_with_deadline(deadline)
}

pub(crate) fn verify_attested_signed_package_from_env_with_deadline(
    custody_deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
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
            verify_attested_signed_package_with_deadline(&input, &plan, &expected, custody_deadline)
        }
        request::VerificationRoute::Admitted => {
            let (plan, expected, input) = publisher_context_from_env(None)?;
            verify_attested_signed_package_with_deadline(&input, &plan, &expected, custody_deadline)
        }
    }
}

pub(crate) fn verify_attested_signed_package(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    verify_attested_signed_package_with_deadline(
        directory,
        plan,
        expected,
        default_verification_deadline()?,
    )
}

pub(crate) fn verify_attested_signed_package_with_deadline(
    directory: &Path,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
    custody_deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    publication::verify_attested_signed_package_with_deadline(
        directory,
        plan,
        expected,
        custody_deadline,
    )
}

fn default_verification_deadline() -> Result<velnor_actions_mise::CheckDeadline, OrchestratorError>
{
    velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(
        u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
    ))
    .map_err(|_| internal("package_verification_deadline"))
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

pub(crate) fn verify_signer_input(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    root: &Path,
) -> Result<crate::package_release_artifacts::VerifiedPackageOutput, OrchestratorError> {
    let input_dir = root.join(SIGNER_INPUT_DIR);
    require_private_directory(&input_dir)?;
    let (version, _, _, _) = release_version(plan, identity)?;
    verify_package_output(
        &input_dir,
        HANDOFF_FILE,
        &root.join(OUTPUT_DIR),
        &plan.products,
        &release_companions(plan),
        PackageOutputIdentity {
            source_commit: &identity.source_sha,
            source_ref: &identity.source_ref,
            version: &version,
        },
    )
}

pub(crate) fn sign_package_release(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    root: &Path,
) -> Result<VerifiedSignedPackage, OrchestratorError> {
    let output_dir = root.join(OUTPUT_DIR);
    plan.validate()
        .map_err(|_| internal("package_signer_plan_invalid"))?;
    validate_identity(plan, identity)?;
    let (version, tag, commit_count, source_sha7) = release_version(plan, identity)?;
    let verified = verify_signer_input(plan, identity, root)?;
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
    let signed = verify_signed_package(&output_dir, plan, identity)?;
    append_attestation_subject_paths(&signed, root)?;
    Ok(VerifiedSignedPackage {
        plan: plan.clone(),
        receipt,
        directory: output_dir,
    })
}

#[path = "package_release_signer_verification.rs"]
mod verification;
pub(crate) use verification::{verify_signed_package, verify_signed_package_with_deadline};
