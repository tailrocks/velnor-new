// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{os::unix::fs::PermissionsExt, path::Path};

use tempfile::Builder;
use velnor_actions_mise::ImageCandidateCommand;

use crate::{OrchestratorError, internal::internal};

use super::super::{InstallBuildxRequest, source, validate::read_lock};
use super::support;

pub(super) fn install_buildx(request: &InstallBuildxRequest) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_candidate_deadline"))?;
    let root = source::checked_root(&request.root)?;
    check_source_identity(&root, request)?;
    let lock = read_lock(&root)?;
    support::verify_buildx_url(&lock.url, &lock.version)?;
    let temp = support::owned_directory(&request.runner_temp)?;
    if temp.starts_with(&root) {
        return Err(internal("image_candidate_temp_inside_source"));
    }
    let plugin_directory = support::private_subdirectory(
        &temp,
        ".velnor-job-image-candidate/docker-config/cli-plugins",
    )?;
    let plugin = plugin_directory.join("docker-buildx");
    if support::path_exists(&plugin)? {
        if support::hash_regular(&plugin, support::MAX_BUILDX_BYTES)? != lock.sha256 {
            return Err(internal("image_candidate_buildx_existing"));
        }
        return Ok(());
    }
    let staging = Builder::new()
        .prefix(".velnor-buildx-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(&temp)
        .map_err(|error| OrchestratorError::io(temp.display().to_string(), error.to_string()))?;
    let staged = staging.path().join("docker-buildx");
    let command = ImageCandidateCommand::download_buildx(&lock.url, &staged)
        .map_err(|_| internal("image_candidate_buildx_command"))?;
    ImageCandidateCommand::run(&command, request.deadline)
        .map_err(|_| internal("image_candidate_buildx_download"))?;
    support::install_verified_buildx(&staged, &plugin, &lock.sha256)
}

fn check_source_identity(
    root: &Path,
    request: &InstallBuildxRequest,
) -> Result<(), OrchestratorError> {
    let head = source::git_text(
        root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        request.deadline,
    )?;
    if head.trim() != source::env("GITHUB_SHA")? {
        return Err(internal("image_candidate_source_identity"));
    }
    source::ensure_clean_checkout(root, request.deadline)
}
