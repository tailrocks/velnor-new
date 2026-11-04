// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_TOOL_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

pub fn read_pr_preparation_archive(archive: &std::fs::File) -> Result<Vec<u8>, OrchestratorError> {
    read_single_file_archive(
        archive,
        "velnor-release-pr-preparation.json",
        64 * 1024 * 1024,
        64 * 1024 * 1024,
    )
}

pub fn validate_pr_preparation_artifact_metadata(
    list: &Value,
    detail: &Value,
    artifact_id: u64,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
    head_sha: &str,
) -> Result<ToolArtifactMetadata, OrchestratorError> {
    validate_artifact_metadata(
        list,
        detail,
        artifact_id,
        run_id,
        attempt,
        repository_id,
        head_sha,
        "pr-preparation",
        64 * 1024 * 1024,
    )
}

pub fn read_receipt_archive(archive: &std::fs::File) -> Result<Vec<u8>, OrchestratorError> {
    read_single_file_archive(
        archive,
        "velnor-crate-release-receipt.json",
        16 * 1024 * 1024,
        1024 * 1024,
    )
}

fn read_single_file_archive(
    archive: &std::fs::File,
    expected: &str,
    archive_limit: u64,
    entry_limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    use std::io::{Read as _, Seek as _};
    let invalid = || internal("crate_release_receipt_archive_invalid");
    let mut input = archive.try_clone().map_err(|_| invalid())?;
    if !input.metadata().map_err(|_| invalid())?.is_file()
        || input.metadata().map_err(|_| invalid())?.len() > archive_limit
    {
        return Err(invalid());
    }
    input.rewind().map_err(|_| invalid())?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(30))
        .map_err(|_| invalid())?;
    crate::check_runtime::preparation::acquisition::archive::preflight_zip_file(
        &mut input, deadline,
    )?;
    input.rewind().map_err(|_| invalid())?;
    let mut zip = zip::ZipArchive::new(input).map_err(|_| invalid())?;
    zip.validate_header_consistency().map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(invalid());
    }
    let mut entry = zip.by_index(0).map_err(|_| invalid())?;
    let mode = entry.unix_mode().unwrap_or(0);
    if entry.name().map_err(|_| invalid())? != expected
        || !entry.is_file()
        || (mode & 0o170_000 != 0 && mode & 0o170_000 != 0o100_000)
        || mode & 0o7000 != 0
        || entry.size() > entry_limit
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(entry_limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > entry_limit || bytes.len() as u64 != entry.size() {
        return Err(invalid());
    }
    Ok(bytes)
}

pub fn validate_receipt_artifact_metadata(
    list: &Value,
    detail: &Value,
    artifact_id: u64,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
    head_sha: &str,
) -> Result<ToolArtifactMetadata, OrchestratorError> {
    validate_artifact_metadata(
        list,
        detail,
        artifact_id,
        run_id,
        attempt,
        repository_id,
        head_sha,
        "receipt",
        16 * 1024 * 1024,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArtifactMetadata {
    pub id: u64,
    pub name: String,
    pub digest: String,
    pub size_in_bytes: u64,
    pub expired: bool,
    pub workflow_run: ToolArtifactRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArtifactRun {
    pub id: u64,
    pub repository_id: u64,
    pub head_repository_id: u64,
    pub head_sha: String,
}

pub fn validate_tool_artifact_metadata(
    list: &Value,
    detail: &Value,
    artifact_id: u64,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
    head_sha: &str,
) -> Result<ToolArtifactMetadata, OrchestratorError> {
    validate_artifact_metadata(
        list,
        detail,
        artifact_id,
        run_id,
        attempt,
        repository_id,
        head_sha,
        "rust",
        MAX_TOOL_ARCHIVE_BYTES,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "artifact identity is independently authenticated"
)]
fn validate_artifact_metadata(
    list: &Value,
    detail: &Value,
    artifact_id: u64,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
    head_sha: &str,
    purpose: &str,
    max_bytes: u64,
) -> Result<ToolArtifactMetadata, OrchestratorError> {
    let invalid = || internal("crate_release_tool_artifact_unverified");
    if artifact_id == 0
        || run_id == 0
        || attempt == 0
        || repository_id == 0
        || head_sha.len() != 40
        || !head_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let entries = list
        .get("artifacts")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if entries.len() >= 100
        || list.get("total_count").and_then(Value::as_u64) != Some(entries.len() as u64)
    {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut selected = None;
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(invalid)?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(invalid)?;
        if !ids.insert(id) || !names.insert(name.to_owned()) {
            return Err(invalid());
        }
        if id == artifact_id {
            selected = Some(entry);
        }
    }
    let selected: ToolArtifactMetadata =
        serde_json::from_value(selected.ok_or_else(invalid)?.clone()).map_err(|_| invalid())?;
    let detail: ToolArtifactMetadata =
        serde_json::from_value(detail.clone()).map_err(|_| invalid())?;
    let digest = detail.digest.strip_prefix("sha256:").ok_or_else(invalid)?;
    if selected != detail
        || detail.id != artifact_id
        || detail.name != format!("velnor-release-{purpose}-{run_id}-{attempt}")
        || detail.expired
        || detail.size_in_bytes == 0
        || detail.size_in_bytes > max_bytes
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || detail.workflow_run.id != run_id
        || detail.workflow_run.repository_id != repository_id
        || detail.workflow_run.head_repository_id != repository_id
        || detail.workflow_run.head_sha != head_sha
    {
        return Err(invalid());
    }
    Ok(detail)
}

pub use crate::fd_tool_extract::extract_verified_tool_archive_file;

fn check_real_directories(path: &std::path::Path) -> Result<(), OrchestratorError> {
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

pub struct ToolchainStageRequest<'a> {
    pub intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    pub github: &'a crate::crate_release_github::GitHubAdmissionRequest,
    pub candidate: &'a crate::crate_release_github::GitHubCandidate,
    pub artifact_id: u64,
    pub token: &'a str,
    pub runner_temp: &'a std::path::Path,
    pub git: &'a crate::crate_release_git::ReleaseGit,
    pub deadline: std::time::Instant,
}

pub fn acquire_reconciliation_receipt_artifact<
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    registry: &R,
    transport: &T,
    source: &std::path::Path,
) -> Result<(), OrchestratorError> {
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source,
        true,
        |receipt| {
            crate::crate_release_semver::verify_preflight(
                source,
                receipt,
                registry,
                request.runner_temp,
                request.git,
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
    source: &std::path::Path,
) -> Result<(), OrchestratorError> {
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source,
        false,
        |receipt| {
            crate::crate_release_semver::verify_preflight(
                source,
                receipt,
                registry,
                request.runner_temp,
                request.git,
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
    source: &std::path::Path,
    reconciliation: bool,
    verify_semver: impl FnOnce(
        &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    ) -> Result<
        std::collections::BTreeMap<String, crate::crate_release_semver::SemverEvidence>,
        OrchestratorError,
    >,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_receipt_acquire_invalid");
    if request.github.run_attempt != 1
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
    crate::crate_release_github::authorize_github_candidate(
        request.github,
        request.candidate,
        github,
    )?;
    if current_artifact(request, github, true)? != initial {
        return Err(invalid());
    }
    let bytes = read_receipt_archive(&file)?;
    let receipt: crate::crate_release_controller::CrateReleaseAdmissionReceipt =
        serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if serde_json::to_vec(&receipt).map_err(|_| invalid())? != bytes
        || receipt.github != *request.candidate
    {
        return Err(invalid());
    }
    if reconciliation {
        let visible = crate::crate_release_controller::validate_reconciliation_receipt(
            &receipt,
            request.intent,
            request.github,
            github,
            registry,
            source,
            request.git,
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
            source,
            request.git,
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
    source: &std::path::Path,
) -> Result<(), OrchestratorError> {
    acquire_receipt(
        request,
        github,
        registry,
        transport,
        source,
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

impl std::fmt::Debug for ToolchainStageRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolchainStageRequest")
            .field("artifact_id", &self.artifact_id)
            .finish_non_exhaustive()
    }
}

pub fn stage_unverified_toolchain<
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    transport: &T,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let invalid = || internal("crate_release_toolchain_staging_invalid");
    request.intent.validate().map_err(|_| invalid())?;
    if request.github.repository != request.intent.repository
        || request.github.default_branch != request.intent.default_branch
        || request.github.environment != request.intent.environment
        || request.github.required_workflow_path != request.intent.required_workflow_path
        || request.github.trusted_reviewer_id != request.intent.trusted_reviewer_id
        || request.github.workflow_path != ".github/workflows/release.yml"
        || request.github.run_attempt != 1
        || request.token.is_empty()
        || request.artifact_id == 0
        || !request.runner_temp.is_absolute()
        || std::time::Instant::now() >= request.deadline
    {
        return Err(invalid());
    }
    let source_matches = match request.github.event.as_str() {
        "workflow_dispatch" => request.intent.recovery.as_ref().is_some_and(|recovery| {
            request.github.source_sha == recovery.source_sha
                && request.github.recovery_sha.as_deref() == Some(recovery.source_sha.as_str())
        }),
        "push" => {
            request.github.recovery_sha.is_none()
                && request.github.source_sha == request.github.workflow_sha
        }
        _ => false,
    };
    if !source_matches {
        return Err(invalid());
    }
    crate::crate_release_github::authorize_github_candidate(
        request.github,
        request.candidate,
        github,
    )?;
    crate::crate_release_github::verify_immutable_tag_rules(&request.intent.repository, github)?;
    let initial = current_artifact(request, github, false)?;
    let parent = request.runner_temp.join("velnor");
    check_real_directories(&parent)?;
    let scratch = tempfile::Builder::new()
        .prefix(".velnor-download-")
        .tempdir_in(&parent)
        .map_err(|_| invalid())?;
    let destination = parent.join("rustup");
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
    crate::crate_release_github::authorize_github_candidate(
        request.github,
        request.candidate,
        github,
    )?;
    crate::crate_release_github::verify_immutable_tag_rules(&request.intent.repository, github)?;
    if current_artifact(request, github, false)? != initial {
        return Err(invalid());
    }
    extract_verified_tool_archive_file(&file, digest, &request.intent.rust_version, &destination)?;
    Ok(destination)
}

fn current_artifact<G: crate::crate_release_github::GitHubAdmissionRead>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    receipt: bool,
) -> Result<ToolArtifactMetadata, OrchestratorError> {
    let invalid = || internal("crate_release_toolchain_staging_invalid");
    let prefix = format!("repos/{}", request.intent.repository);
    let repository = github.get(&prefix)?;
    if repository["full_name"].as_str() != Some(request.intent.repository.as_str()) {
        return Err(invalid());
    }
    let repository_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    let run = github.get(&format!("{prefix}/actions/runs/{}", request.github.run_id))?;
    if run["id"].as_u64() != Some(request.github.run_id)
        || run["run_attempt"].as_u64() != Some(1)
        || run["head_sha"].as_str() != Some(request.github.workflow_sha.as_str())
        || run["repository"]["id"].as_u64() != Some(repository_id)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
    {
        return Err(invalid());
    }
    let list = github.get(&format!(
        "{prefix}/actions/runs/{}/artifacts?per_page=100",
        request.github.run_id
    ))?;
    let detail = github.get(&format!(
        "{prefix}/actions/artifacts/{}",
        request.artifact_id
    ))?;
    let validate = if receipt {
        validate_receipt_artifact_metadata
    } else {
        validate_tool_artifact_metadata
    };
    validate(
        &list,
        &detail,
        request.artifact_id,
        request.github.run_id,
        1,
        repository_id,
        &request.github.workflow_sha,
    )
}
