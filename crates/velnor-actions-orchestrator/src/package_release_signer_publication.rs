use std::path::Path;

use crate::OrchestratorError;

pub(super) fn verify_attested_signed_package_with_deadline(
    directory: &Path,
    plan: &velnor_actions_contract::PackageReleasePlan,
    expected: &super::PackageReleaseSigningIdentity,
    custody_deadline: velnor_actions_mise::CheckDeadline,
) -> Result<super::VerifiedAttestedSignedPackage, OrchestratorError> {
    let (signed, closure) =
        hold_signed_directory_for_publication(directory, custody_deadline, || {
            verify_signed_and_attestation(directory, plan, expected)
        })?;
    Ok(super::VerifiedAttestedSignedPackage {
        signed,
        closure: std::sync::Arc::new(closure),
        identity: expected.clone(),
    })
}

pub(crate) fn verify_attested_signed_package_with_owned_tempdir(
    owner: tempfile::TempDir,
    plan: &velnor_actions_contract::PackageReleasePlan,
    expected: &super::PackageReleaseSigningIdentity,
    custody_deadline: velnor_actions_mise::CheckDeadline,
) -> Result<super::VerifiedAttestedSignedPackage, OrchestratorError> {
    let directory = owner.path().to_path_buf();
    let (signed, closure) =
        hold_owned_signed_directory_for_publication(owner, custody_deadline, || {
            verify_signed_and_attestation(&directory, plan, expected)
        })?;
    Ok(super::VerifiedAttestedSignedPackage {
        signed,
        closure: std::sync::Arc::new(closure),
        identity: expected.clone(),
    })
}

fn verify_signed_and_attestation(
    directory: &Path,
    plan: &velnor_actions_contract::PackageReleasePlan,
    expected: &super::PackageReleaseSigningIdentity,
) -> Result<super::VerifiedSignedPackage, OrchestratorError> {
    let mut signed = super::verify_signed_package(directory, plan, expected)?;
    super::attestation::verify_release_attestation(&signed, expected)?;
    signed.plan = plan.clone();
    Ok(signed)
}

pub(super) fn hold_signed_directory_for_publication<T>(
    directory: &Path,
    custody_deadline: velnor_actions_mise::CheckDeadline,
    verify: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<(T, crate::filesystem_owner::held_directory::HeldDirectory), OrchestratorError> {
    crate::package_release_publish::budget::with_default_scope(|_| {
        crate::package_release_publish::budget::hold_signed_directory(
            directory,
            custody_deadline,
            verify,
        )
    })
}

pub(crate) fn hold_owned_signed_directory_for_publication<T>(
    owner: tempfile::TempDir,
    custody_deadline: velnor_actions_mise::CheckDeadline,
    verify: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<(T, crate::filesystem_owner::held_directory::HeldDirectory), OrchestratorError> {
    crate::package_release_publish::budget::with_default_scope(|_| {
        crate::filesystem_owner::held_directory::HeldDirectory::admit_owned(
            owner,
            custody_deadline,
            verify,
        )
    })
}
