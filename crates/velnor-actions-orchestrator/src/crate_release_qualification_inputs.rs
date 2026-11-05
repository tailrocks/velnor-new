// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Component, Path};
use velnor_actions_mise::GitRequest;

use crate::{
    OrchestratorError,
    crate_release_git::ReleaseGit,
    internal::internal,
    safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file},
};

#[derive(Debug, Clone, Copy)]
pub struct CheckoutInputRequest<'a> {
    pub control: &'a Path,
    pub control_sha: &'a str,
    pub source: &'a Path,
    pub source_sha: &'a str,
    pub config_path: &'a str,
    pub manifest_path: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckoutInputs {
    config: String,
    manifest: String,
    control_sha: String,
    source_sha: String,
    config_path: String,
    manifest_path: String,
}

impl CheckoutInputs {
    #[must_use]
    pub fn config(&self) -> &str {
        &self.config
    }
    #[must_use]
    pub fn manifest(&self) -> &str {
        &self.manifest
    }
    #[must_use]
    pub fn control_sha(&self) -> &str {
        &self.control_sha
    }
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.source_sha
    }
    #[must_use]
    pub fn config_path(&self) -> &str {
        &self.config_path
    }
    #[must_use]
    pub fn manifest_path(&self) -> &str {
        &self.manifest_path
    }
}

pub fn capture_checkout_inputs(
    request: &CheckoutInputRequest<'_>,
    git: &ReleaseGit,
) -> Result<CheckoutInputs, OrchestratorError> {
    let first = observe(request, git)?;
    let second = observe(request, git)?;
    if first != second {
        return Err(invalid());
    }
    Ok(first)
}

fn observe(
    request: &CheckoutInputRequest<'_>,
    git: &ReleaseGit,
) -> Result<CheckoutInputs, OrchestratorError> {
    crate::verify_crate_release_source(request.control, request.control_sha, git)?;
    crate::verify_crate_release_source(request.source, request.source_sha, git)?;
    let config = read(
        request.control,
        request.control_sha,
        request.config_path,
        git,
    )?;
    let manifest = read(
        request.control,
        request.control_sha,
        request.manifest_path,
        git,
    )?;
    crate::verify_crate_release_source(request.control, request.control_sha, git)?;
    crate::verify_crate_release_source(request.source, request.source_sha, git)?;
    Ok(CheckoutInputs {
        config,
        manifest,
        control_sha: request.control_sha.to_owned(),
        source_sha: request.source_sha.to_owned(),
        config_path: request.config_path.to_owned(),
        manifest_path: request.manifest_path.to_owned(),
    })
}

fn read(root: &Path, sha: &str, path: &str, git: &ReleaseGit) -> Result<String, OrchestratorError> {
    if path.is_empty()
        || !Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid());
    }
    let output = GitRequest::ls_tree(vec![
        "-z".into(),
        "--full-tree".into(),
        sha.into(),
        "--".into(),
        format!(":(literal){path}").into(),
    ])
    .run_release_in(root, git)
    .map_err(|_| invalid())?;
    if !output.success {
        return Err(invalid());
    }
    let record = std::str::from_utf8(&output.stdout).map_err(|_| invalid())?;
    let (head, listed) = record
        .strip_suffix('\0')
        .ok_or_else(invalid)?
        .split_once('\t')
        .ok_or_else(invalid)?;
    let mut fields = head.split(' ');
    let mode = fields.next().ok_or_else(invalid)?;
    let kind = fields.next().ok_or_else(invalid)?;
    let object = fields.next().ok_or_else(invalid)?;
    if fields.next().is_some()
        || !matches!(mode, "100644" | "100755")
        || kind != "blob"
        || listed != path
    {
        return Err(invalid());
    }
    let value = match read_repo_file(root, path, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(value) => Ok(value),
        RepoRead::Absent => Err(invalid()),
    }?;
    if crate::release_policy_admission::git_blob_sha1(value.as_bytes())? != object {
        return Err(invalid());
    }
    Ok(value)
}

fn invalid() -> OrchestratorError {
    internal("qualification_checkout_inputs_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_inputs.rs"]
mod tests;
