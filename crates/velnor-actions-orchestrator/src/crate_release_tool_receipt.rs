// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ToolchainStageRequest, current_artifact, read_receipt_archive};
use crate::{OrchestratorError, internal::internal};

pub fn acquire_reconciliation_receipt_artifact<
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    registry: &R,
    transport: &T,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    let source = source_context.root;
    let source_execution = source_context.execution;
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source_context,
        true,
        |receipt| {
            crate::crate_release_semver::verify_preflight(
                source,
                receipt,
                registry,
                request.runner_temp,
                source_context.git,
                source_execution,
            )
        },
    )
}

pub fn acquire_receipt_artifact<
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    registry: &R,
    transport: &T,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    let source = source_context.root;
    let source_execution = source_context.execution;
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source_context,
        false,
        |receipt| {
            crate::crate_release_semver::verify_preflight(
                source,
                receipt,
                registry,
                request.runner_temp,
                source_context.git,
                source_execution,
            )
        },
    )
}

fn acquire_receipt<
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    registry: &R,
    transport: &T,
    source_context: crate::CrateSource<'_>,
    reconciliation: bool,
    verify_semver: impl FnOnce(
        &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    ) -> Result<
        std::collections::BTreeMap<String, crate::crate_release_semver::SemverEvidence>,
        OrchestratorError,
    >,
) -> Result<(), OrchestratorError> {
    let source = source_context.root;
    let source_execution = source_context.execution;
    let authenticated = acquire_authenticated_receipt(request, github, transport)?;
    let (bytes, receipt, _authority) = authenticated.into_parts();
    let invalid = || internal("crate_release_receipt_acquire_invalid");
    if reconciliation {
        let visible = crate::crate_release_controller::validate_reconciliation_receipt(
            &receipt,
            request.intent,
            request.github,
            github,
            registry,
            crate::CrateSource::new(source, source_context.git, source_execution),
        )?;
        if !visible {
            return Err(internal("crate_release_visibility_timeout"));
        }
    } else {
        let mut fresh = crate::crate_release_controller::prepare_crate_release(
            request.intent,
            request.github,
            github,
            registry,
            crate::CrateSource::new(source, source_context.git, source_execution),
        )?;
        fresh.semver_evidence = verify_semver(&fresh)?;
        crate::crate_release_controller::verify_observed_packages(&fresh, &receipt)?;
        if !crate::crate_release_semver::matches_revalidated_preflight(
            &receipt.semver_evidence,
            &fresh.semver_evidence,
        ) {
            return Err(invalid());
        }
    }
    write_receipt(request.runner_temp, &bytes)
}

pub(crate) struct AuthenticatedReceipt {
    bytes: Vec<u8>,
    receipt: crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    authority: crate::crate_release_github::GitHubAuthorized,
}

impl AuthenticatedReceipt {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<u8>,
        crate::crate_release_controller::CrateReleaseAdmissionReceipt,
        crate::crate_release_github::GitHubAuthorized,
    ) {
        (self.bytes, self.receipt, self.authority)
    }
}

pub(crate) fn acquire_authenticated_receipt<
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    transport: &T,
) -> Result<AuthenticatedReceipt, OrchestratorError> {
    let invalid = || internal("crate_release_receipt_acquire_invalid");
    if !super::staging::authority_matches(request)
        || request.token.is_empty()
        || request.artifact_id == 0
        || !request.runner_temp.is_absolute()
        || std::time::Instant::now() >= request.deadline
    {
        return Err(invalid());
    }
    crate::crate_release_github::authorize_github_candidate(
        request.github,
        request.candidate,
        github,
    )?;
    let initial = current_artifact(request, github, true)?;
    check_real_directories(request.runner_temp)?;
    let scratch = tempfile::Builder::new()
        .prefix(".velnor-receipt-")
        .tempdir_in(request.runner_temp)
        .map_err(|_| invalid())?;
    let digest = initial.digest.strip_prefix("sha256:").ok_or_else(invalid)?;
    let file = crate::crate_release_tool_download::download_tool_artifact(
        transport,
        &crate::crate_release_tool_download::ToolDownloadRequest {
            repository: &request.intent.repository,
            id: request.artifact_id,
            token: request.token,
            expected_size: initial.size_in_bytes,
            expected_sha256: digest,
            destination: &scratch.path().join("archive.zip"),
            deadline: request.deadline,
        },
    )?;
    let authority = crate::crate_release_github::authorize_github_candidate(
        request.github,
        request.candidate,
        github,
    )?;
    if current_artifact(request, github, true)? != initial {
        return Err(invalid());
    }
    let bytes = read_receipt_archive(&file)?;
    let receipt = decode_receipt(&bytes, request.candidate)?;
    if receipt.schema != 1 || receipt.intent != *request.intent {
        return Err(invalid());
    }
    Ok(AuthenticatedReceipt {
        bytes,
        receipt,
        authority,
    })
}

#[cfg(test)]
pub(crate) fn fixture_authenticate_receipt<
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    transport: &T,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, OrchestratorError> {
    acquire_authenticated_receipt(request, github, transport).map(|value| value.receipt)
}

fn decode_receipt(
    bytes: &[u8],
    candidate: &crate::crate_release_github::GitHubCandidate,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, OrchestratorError> {
    let invalid = || internal("crate_release_receipt_acquire_invalid");
    let receipt: crate::crate_release_controller::CrateReleaseAdmissionReceipt =
        serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if serde_json::to_vec(&receipt).map_err(|_| invalid())? != bytes || receipt.github != *candidate
    {
        return Err(invalid());
    }
    Ok(receipt)
}

#[cfg(test)]
pub(crate) fn fixture_acquire_receipt<
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    registry: &R,
    transport: &T,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source_context,
        false,
        |receipt| {
            Ok(receipt
                .packages
                .iter()
                .map(|p| {
                    (
                        p.source.name.clone(),
                        crate::crate_release_semver::SemverEvidence::NoBaseline {
                            checker_sha256:
                                velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                                    "ubuntu-24.04",
                                )
                                .expect("qualified checker")
                                .executable_sha256
                                .into(),
                        },
                    )
                })
                .collect())
        },
    )
}

#[cfg(unix)]
pub(crate) fn write_receipt(
    parent: &std::path::Path,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    use rustix::fs::{AtFlags, Mode, OFlags, openat, unlinkat};
    use std::io::Write as _;
    let invalid = || internal("crate_release_receipt_write");
    let parent = crate::crate_release_tool_download::open_download_parent(parent)?;
    let name = "velnor-crate-release-receipt.json";
    let fd = openat(
        &parent,
        name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| invalid())?;
    let mut file = std::fs::File::from(fd);
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        unlinkat(&parent, name, AtFlags::empty()).map_err(|_| invalid())?;
        return Err(invalid());
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn write_receipt(
    _parent: &std::path::Path,
    _bytes: &[u8],
) -> Result<(), OrchestratorError> {
    Err(internal("crate_release_receipt_platform_unsupported"))
}

pub(super) fn check_real_directories(path: &std::path::Path) -> Result<(), OrchestratorError> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                current.push(component);
            }
            _ => return Err(internal("crate_release_tool_archive_invalid")),
        }
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| internal("crate_release_tool_archive_invalid"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(internal("crate_release_tool_archive_invalid"));
        }
    }
    Ok(())
}
