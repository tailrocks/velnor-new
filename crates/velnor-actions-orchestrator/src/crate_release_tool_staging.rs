// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    OrchestratorError, ToolArtifactMetadata, ToolchainStageRequest, check_real_directories,
    extract_verified_tool_archive_file, internal, validate_receipt_artifact_metadata,
    validate_tool_artifact_metadata,
};

pub fn stage_unverified_toolchain<
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
>(
    request: &ToolchainStageRequest<'_>,
    github: &G,
    transport: &T,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let invalid = || internal("crate_release_toolchain_staging_invalid");
    if !authority_matches(request)
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

pub(super) fn authority_matches(request: &ToolchainStageRequest<'_>) -> bool {
    request.intent.validate().is_ok()
        && request.github.repository == request.intent.repository
        && request.github.default_branch == request.intent.default_branch
        && request.github.environment == request.intent.environment
        && request.github.required_workflow_path == request.intent.required_workflow_path
        && request.github.trusted_reviewer_id == request.intent.trusted_reviewer_id
        && request.github.workflow_path == ".github/workflows/release.yml"
        && request.github.run_attempt == 1
        && source_matches(request)
}

fn source_matches(request: &ToolchainStageRequest<'_>) -> bool {
    match request.github.event.as_str() {
        "workflow_dispatch" => request
            .intent
            .recovery
            .as_ref()
            .map(|recovery| recovery.source_sha.as_str())
            .or_else(|| {
                request
                    .intent
                    .new_crates
                    .as_ref()
                    .map(|creation| creation.source_sha.as_str())
            })
            .is_some_and(|source| {
                request.github.source_sha == source
                    && request.github.recovery_sha.as_deref() == Some(source)
            }),
        "push" => {
            request.intent.new_crates.is_none()
                && request.github.recovery_sha.is_none()
                && request.github.source_sha == request.github.workflow_sha
        }
        _ => false,
    }
}

pub(super) fn current_artifact<G: crate::crate_release_github::GitHubAdmissionRead>(
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
