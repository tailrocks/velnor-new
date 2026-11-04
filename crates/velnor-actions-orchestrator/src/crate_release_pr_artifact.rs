// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_mise::GitRequest;

const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationBinding {
    pub repository: String,
    pub workflow_sha: String,
    pub source_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedFile {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationArtifact {
    pub schema: u32,
    pub binding: PreparationBinding,
    pub files: Vec<PreparedFile>,
}

#[derive(Debug)]
pub struct PreparationCheckout {
    root: PathBuf,
    git: crate::crate_release_git::ReleaseGit,
    binding: PreparationBinding,
    allowed_paths: BTreeSet<String>,
    tracked: BTreeMap<String, TrackedFile>,
    git_files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedFile {
    mode: String,
    oid: String,
}

impl PreparationCheckout {
    pub fn open(
        root: &Path,
        binding: PreparationBinding,
        allowed_paths: BTreeSet<String>,
        git: crate::crate_release_git::ReleaseGit,
    ) -> Result<Self, &'static str> {
        if !valid_binding(&binding) || allowed_paths.iter().any(|path| !valid_path(path)) {
            return Err("release_pr_checkout_invalid");
        }
        crate::verify_crate_release_source(root, &binding.source_sha, &git)
            .map_err(|_| "release_pr_checkout_invalid")?;
        let root = root
            .canonicalize()
            .map_err(|_| "release_pr_checkout_invalid")?;
        let tracked = tracked_files(&root, &git)?;
        let git_files = git_file_digests(&root, &git)?;
        Ok(Self {
            root,
            git,
            binding,
            allowed_paths,
            tracked,
            git_files,
        })
    }

    pub fn collect(self) -> Result<PreparationArtifact, &'static str> {
        self.collect_with(|| {})
    }

    fn collect_with(self, between: impl FnOnce()) -> Result<PreparationArtifact, &'static str> {
        self.ensure_current()?;
        let changed = self.capture_changes()?;
        between();
        self.ensure_current()?;
        if self.capture_changes()? != changed {
            return Err("release_pr_checkout_changed");
        }
        self.ensure_current()?;
        let artifact = PreparationArtifact {
            schema: 1,
            binding: self.binding,
            files: changed.into_values().collect(),
        };
        let bytes = serde_json::to_vec(&artifact).map_err(|_| "release_pr_artifact_invalid")?;
        verify_preparation_artifact(
            &bytes,
            &sha256(&bytes),
            &artifact.binding,
            &self.allowed_paths,
        )
    }

    fn ensure_current(&self) -> Result<(), &'static str> {
        let head = git_output(
            &self.root,
            &GitRequest::rev_parse(vec!["HEAD".into()]),
            &self.git,
        )?;
        if head != format!("{}\n", self.binding.source_sha).as_bytes()
            || tracked_files(&self.root, &self.git)? != self.tracked
            || git_file_digests(&self.root, &self.git)? != self.git_files
        {
            return Err("release_pr_checkout_changed");
        }
        Ok(())
    }

    fn capture_changes(&self) -> Result<BTreeMap<String, PreparedFile>, &'static str> {
        let mut changed = BTreeMap::new();
        let mut changed_bytes = 0usize;
        for (path, tracked) in &self.tracked {
            let bytes = read_tracked(&self.root, path, &tracked.mode)?;
            let oid = crate::release_policy_admission::git_blob_sha1(&bytes)
                .map_err(|_| "release_pr_checkout_invalid")?;
            if oid != tracked.oid {
                if !self.allowed_paths.contains(path) || tracked.mode == "120000" {
                    return Err("release_pr_checkout_unapproved_change");
                }
                insert_change(&mut changed, &mut changed_bytes, path, bytes)?;
            }
        }
        for path in untracked_files(&self.root, &self.git)? {
            if !self.allowed_paths.contains(&path) {
                return Err("release_pr_checkout_unapproved_change");
            }
            let bytes = read_tracked(&self.root, &path, "100644")?;
            insert_change(&mut changed, &mut changed_bytes, &path, bytes)?;
        }
        Ok(changed)
    }
}

fn insert_change(
    changed: &mut BTreeMap<String, PreparedFile>,
    total: &mut usize,
    path: &str,
    bytes: Vec<u8>,
) -> Result<(), &'static str> {
    *total = total
        .checked_add(bytes.len())
        .ok_or("release_pr_artifact_invalid")?;
    if bytes.len() > MAX_FILE_BYTES || *total > MAX_TOTAL_BYTES || changed.len() >= MAX_FILES {
        return Err("release_pr_artifact_invalid");
    }
    if changed
        .insert(
            path.to_owned(),
            PreparedFile {
                path: path.to_owned(),
                sha256: sha256(&bytes),
                bytes,
            },
        )
        .is_some()
    {
        return Err("release_pr_artifact_invalid");
    }
    Ok(())
}

#[path = "crate_release_pr_artifact_files.rs"]
mod files;
use files::{
    git_file_digests, git_output, read_regular, read_tracked, tracked_files, untracked_files,
};

pub fn verify_preparation_artifact(
    bytes: &[u8],
    digest: &str,
    binding: &PreparationBinding,
    allowed_paths: &BTreeSet<String>,
) -> Result<PreparationArtifact, &'static str> {
    let invalid = || "release_pr_artifact_invalid";
    if bytes.len() > MAX_ARTIFACT_BYTES || digest != sha256(bytes) || !valid_binding(binding) {
        return Err(invalid());
    }
    let artifact: PreparationArtifact = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if artifact.schema != 1 || artifact.binding != *binding || artifact.files.len() > MAX_FILES {
        return Err(invalid());
    }
    let mut previous: Option<&str> = None;
    let mut total = 0usize;
    for file in &artifact.files {
        if !valid_path(&file.path)
            || !allowed_paths.contains(&file.path)
            || previous.is_some_and(|prior| prior >= file.path.as_str())
            || file.bytes.len() > MAX_FILE_BYTES
            || file.sha256 != sha256(&file.bytes)
        {
            return Err(invalid());
        }
        total = total.checked_add(file.bytes.len()).ok_or_else(invalid)?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid());
        }
        previous = Some(&file.path);
    }
    Ok(artifact)
}

fn valid_binding(binding: &PreparationBinding) -> bool {
    let Some((owner, repository)) = binding.repository.split_once('/') else {
        return false;
    };
    velnor_actions_contract::config::is_valid_github_owner(owner)
        && velnor_actions_contract::config::is_valid_github_repo_name(repository)
        && valid_git_sha(&binding.workflow_sha)
        && valid_git_sha(&binding.source_sha)
        && binding.run_id > 0
        && binding.run_attempt > 0
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.bytes().any(|byte| byte.is_ascii_control())
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

#[path = "crate_release_pr_artifact_prepare.rs"]
mod prepare;
#[cfg(test)]
pub(crate) use prepare::fixture_execute_preparation_command;
pub use prepare::prepare_pr_from_environment;

#[cfg(test)]
#[path = "crate_release_pr_artifact_tests.rs"]
mod tests;
