//! Safe staging and typed command adapter for the fixed Mise Cosign boundary.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use velnor_actions_contract::package_release::{PackageReleaseAsset, PackageReleaseAssetKind};
use velnor_actions_mise::{
    COSIGN_COMMAND_OUTPUT_LIMIT, COSIGN_COMMAND_TIMEOUT, COSIGN_LINUX_AMD64_SHA256,
    COSIGN_MAX_BYTES, COSIGN_VERSION, CosignCertificateIdentity, PinnedCosign,
};

use super::{
    PackageReleaseSigningIdentity, VerifiedPackageArtifact,
    runtime::{read_regular_file, sha256, write_new_file},
};
use crate::{OrchestratorError, internal::internal};

const MAX_COSIGN_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn acquire_cosign(runner_temp: &Path) -> Result<PinnedCosign, OrchestratorError> {
    if !runner_temp.is_absolute() {
        return Err(internal("cosign_runner_temp_invalid"));
    }
    let root = runner_temp.join("velnor");
    ensure_private_directory(&root)?;
    let tools = root.join("tools");
    ensure_private_directory(&tools)?;
    let binary = tools.join(format!("cosign-{COSIGN_VERSION}"));
    if path_exists(&binary) {
        verify_staged_binary(&binary)?;
        return PinnedCosign::from_verified_path(&binary)
            .map_err(|_| internal("cosign_binary_admission_failed"));
    }

    let output = PinnedCosign::download_command()
        .map_err(|_| internal("cosign_download_command_invalid"))?
        .run_bounded(COSIGN_MAX_BYTES, COSIGN_COMMAND_TIMEOUT)
        .map_err(|_| internal("cosign_download_failed"))?;
    if !output.success || !valid_cosign_bytes(&output.stdout) {
        return Err(internal("cosign_download_digest_mismatch"));
    }

    let temporary = tools.join(format!("cosign-{COSIGN_VERSION}.download"));
    remove_stale_temporary(&temporary)?;
    write_new_file(&temporary, &output.stdout)?;
    set_executable_mode(&temporary)?;
    fs::hard_link(&temporary, &binary).map_err(|_| internal("cosign_install_conflict"))?;
    fs::remove_file(&temporary).map_err(|_| internal("cosign_download_cleanup"))?;
    verify_staged_binary(&binary)?;
    PinnedCosign::from_verified_path(&binary)
        .map_err(|_| internal("cosign_binary_admission_failed"))
}

pub(super) fn sign_and_verify_blob(
    cosign: &PinnedCosign,
    artifact: &VerifiedPackageArtifact,
    expected: &PackageReleaseSigningIdentity,
) -> Result<VerifiedPackageArtifact, OrchestratorError> {
    let bundle_name = format!("{}.bundle", artifact.name);
    let bundle_path = artifact
        .path
        .parent()
        .ok_or_else(|| internal("cosign_artifact_parent_missing"))?
        .join(&bundle_name);
    let certificate = certificate_identity(expected)?;
    run_cosign(
        &cosign
            .sign_blob_command(&artifact.path, &bundle_path)
            .map_err(|_| internal("cosign_sign_command_invalid"))?,
    )?;
    run_cosign(
        &cosign
            .verify_blob_command(&artifact.path, &bundle_path, &certificate)
            .map_err(|_| internal("cosign_verify_command_invalid"))?,
    )?;
    signed_bundle_artifact(&bundle_name, &bundle_path, artifact)
}

pub(super) fn verify_blob(
    cosign: &PinnedCosign,
    artifact: &Path,
    bundle: &Path,
    expected: &PackageReleaseSigningIdentity,
) -> Result<(), OrchestratorError> {
    let certificate = certificate_identity(expected)?;
    let command = cosign
        .verify_blob_command(artifact, bundle, &certificate)
        .map_err(|_| internal("cosign_verify_command_invalid"))?;
    run_cosign(&command)
}

fn run_cosign(command: &velnor_actions_mise::IsolatedCommand) -> Result<(), OrchestratorError> {
    let output = command
        .run_bounded(COSIGN_COMMAND_OUTPUT_LIMIT, COSIGN_COMMAND_TIMEOUT)
        .map_err(|_| internal("cosign_operation_failed"))?;
    if output.success {
        Ok(())
    } else {
        Err(internal("cosign_operation_failed"))
    }
}

fn certificate_identity(
    identity: &PackageReleaseSigningIdentity,
) -> Result<CosignCertificateIdentity, OrchestratorError> {
    CosignCertificateIdentity::new(
        &identity.repository,
        &identity.workflow_path,
        &identity.source_ref,
        &identity.workflow_sha,
        identity.event.as_str(),
    )
    .map_err(|_| internal("cosign_certificate_identity_invalid"))
}

fn signed_bundle_artifact(
    name: &str,
    path: &Path,
    subject: &VerifiedPackageArtifact,
) -> Result<VerifiedPackageArtifact, OrchestratorError> {
    let bytes = read_regular_file(path, MAX_COSIGN_BUNDLE_BYTES)?;
    let size = bytes.len() as u64;
    let digest = sha256(&bytes);
    let companion = subject.release_asset.kind == PackageReleaseAssetKind::Companion;
    let kind = if companion {
        PackageReleaseAssetKind::CompanionSignatureBundle
    } else {
        PackageReleaseAssetKind::SignatureBundle
    };
    Ok(VerifiedPackageArtifact {
        name: name.to_owned(),
        path: path.to_path_buf(),
        size,
        sha256: digest.clone(),
        release_asset: PackageReleaseAsset {
            name: name.to_owned(),
            kind,
            product_id: subject.release_asset.product_id.clone(),
            target: subject.release_asset.target.clone(),
            subject_name: Some(subject.name.clone()),
            size,
            sha256: digest,
        },
    })
}

fn valid_cosign_bytes(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.len() <= COSIGN_MAX_BYTES
        && sha256(bytes) == COSIGN_LINUX_AMD64_SHA256
}

fn verify_staged_binary(path: &Path) -> Result<(), OrchestratorError> {
    let bytes = read_regular_file(path, COSIGN_MAX_BYTES as u64)?;
    if !valid_cosign_bytes(&bytes) {
        return Err(internal("cosign_staged_binary_digest_mismatch"));
    }
    set_executable_mode(path)
}

fn ensure_private_directory(path: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(internal("cosign_directory_invalid"))
        }
        Ok(_) => set_directory_mode(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| internal("cosign_directory_create_failed"))?;
            set_directory_mode(path)
        }
        Err(_) => Err(internal("cosign_directory_invalid")),
    }
}

fn remove_stale_temporary(path: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(internal("cosign_temporary_invalid"))
        }
        Ok(_) => fs::remove_file(path).map_err(|_| internal("cosign_temporary_cleanup")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(internal("cosign_temporary_invalid")),
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(unix)]
fn set_directory_mode(path: &Path) -> Result<(), OrchestratorError> {
    let mut permissions = fs::metadata(path)
        .map_err(|_| internal("cosign_directory_permissions_failed"))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
        .map_err(|_| internal("cosign_directory_permissions_failed"))
}

#[cfg(not(unix))]
fn set_directory_mode(_path: &Path) -> Result<(), OrchestratorError> {
    Err(internal("cosign_platform_unsupported"))
}

#[cfg(unix)]
fn set_executable_mode(path: &Path) -> Result<(), OrchestratorError> {
    let mut permissions = fs::metadata(path)
        .map_err(|_| internal("cosign_binary_permissions_failed"))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).map_err(|_| internal("cosign_binary_permissions_failed"))
}

#[cfg(not(unix))]
fn set_executable_mode(_path: &Path) -> Result<(), OrchestratorError> {
    Err(internal("cosign_platform_unsupported"))
}

#[cfg(test)]
#[path = "package_release_signer_cosign_tests.rs"]
mod tests;
