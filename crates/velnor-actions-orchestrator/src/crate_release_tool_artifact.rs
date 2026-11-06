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
    if entries.len() > 10_000
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

pub struct ToolchainStageRequest<'a> {
    pub intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    pub github: &'a crate::crate_release_github::GitHubAdmissionRequest,
    pub candidate: &'a crate::crate_release_github::GitHubCandidate,
    pub artifact_id: u64,
    pub token: &'a str,
    pub runner_temp: &'a std::path::Path,
    pub deadline: std::time::Instant,
}

#[path = "crate_release_tool_receipt.rs"]
pub(crate) mod receipt;
use receipt::check_real_directories;
pub(crate) use receipt::write_receipt;
pub use receipt::{acquire_receipt_artifact, acquire_reconciliation_receipt_artifact};
#[cfg(test)]
pub(crate) use receipt::{fixture_acquire_receipt, fixture_authenticate_receipt};

impl std::fmt::Debug for ToolchainStageRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolchainStageRequest")
            .field("artifact_id", &self.artifact_id)
            .finish_non_exhaustive()
    }
}

#[path = "crate_release_tool_staging.rs"]
mod staging;
use staging::current_artifact;
pub use staging::{StagedToolchain, stage_unverified_toolchain};

#[path = "crate_release_tool_retention.rs"]
mod retention;
pub(crate) use retention::verify_preflight_retention_now;
