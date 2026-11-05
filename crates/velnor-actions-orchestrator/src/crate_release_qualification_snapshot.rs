// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, crate_release_git::ReleaseGit, internal::internal};
use std::path::Path;

#[derive(Debug)]
pub struct SourceSnapshot {
    directory: tempfile::TempDir,
    source_sha: String,
    tree_sha: String,
    manifests: std::collections::BTreeSet<String>,
    nested_toolchains: Vec<String>,
}

impl SourceSnapshot {
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        self.directory.path()
    }
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.source_sha
    }
    #[must_use]
    pub fn tree_sha(&self) -> &str {
        &self.tree_sha
    }
    pub(crate) fn manifests(&self) -> &std::collections::BTreeSet<String> {
        &self.manifests
    }
    pub(crate) fn nested_toolchains(&self) -> &[String] {
        &self.nested_toolchains
    }
}

pub fn snapshot_approved_source(
    source: &Path,
    sha: &str,
    git: &ReleaseGit,
) -> Result<SourceSnapshot, OrchestratorError> {
    use crate::crate_release_journal::JournalFileKind;
    let inventory = crate::journal_source_inventory(source, sha, git)?;
    let tree = velnor_actions_mise::GitRequest::rev_parse(vec![format!("{sha}^{{tree}}").into()])
        .run_release_in(source, git)
        .map_err(|_| invalid())?;
    if !tree.success {
        return Err(invalid());
    }
    let tree_sha = tree
        .stdout_text("git")
        .map_err(|_| invalid())?
        .trim()
        .to_owned();
    if tree_sha.len() != 40
        || !tree_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let directory = tempfile::tempdir().map_err(|_| invalid())?;
    let mut manifests = std::collections::BTreeSet::new();
    let mut nested_toolchains = Vec::new();
    let mut links = Vec::new();
    for entry in &inventory {
        let path = directory.path().join(&entry.path);
        if entry
            .path
            .rsplit_once('/')
            .is_some_and(|(_, name)| matches!(name, "rust-toolchain" | "rust-toolchain.toml"))
        {
            nested_toolchains.push(entry.path.clone());
        }
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            manifests.insert(entry.path.clone());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| invalid())?;
        }
        match &entry.kind {
            JournalFileKind::Regular { executable } => {
                copy_regular(source, entry, &path, *executable)?;
            }
            JournalFileKind::Symlink { target } => links.push((path, target.clone())),
        }
    }
    for (path, target) in &links {
        if Path::new(target).is_absolute() {
            return Err(invalid());
        }
        create_link(path, target)?;
    }
    let snapshot_root = directory.path().canonicalize().map_err(|_| invalid())?;
    for (path, _) in &links {
        let target = path.canonicalize().map_err(|_| invalid())?;
        if !target.starts_with(&snapshot_root) || !target.is_file() {
            return Err(invalid());
        }
    }
    crate::verify_crate_release_source(source, sha, git)?;
    Ok(SourceSnapshot {
        directory,
        source_sha: sha.to_owned(),
        tree_sha,
        manifests,
        nested_toolchains,
    })
}

fn copy_regular(
    source: &Path,
    entry: &crate::crate_release_journal::JournalFile,
    path: &Path,
    executable: bool,
) -> Result<(), OrchestratorError> {
    use std::io::Write as _;
    let bytes = match crate::safe_read::read_repo_bytes(source, &entry.path, 256 * 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => return Err(invalid()),
    };
    if crate::generator::sha256_hex(&bytes) != entry.sha256 {
        return Err(invalid());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| invalid())?;
    file.write_all(&bytes).map_err(|_| invalid())?;
    set_executable(path, executable)
}

#[cfg(unix)]
fn create_link(path: &Path, target: &str) -> Result<(), OrchestratorError> {
    std::os::unix::fs::symlink(target, path).map_err(|_| invalid())
}
#[cfg(not(unix))]
fn create_link(_path: &Path, _target: &str) -> Result<(), OrchestratorError> {
    Err(invalid())
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
    )
    .map_err(|_| invalid())
}
#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<(), OrchestratorError> {
    Err(invalid())
}

fn invalid() -> OrchestratorError {
    internal("qualification_source_snapshot_invalid")
}
