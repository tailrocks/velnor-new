// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus, run_command_bounded};

use super::CollectorError;
use super::filesystem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    device: u64,
    inode: u64,
    links: u64,
}

impl From<&Metadata> for Identity {
    fn from(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
        }
    }
}

pub(super) struct Guard {
    path: PathBuf,
    parent: PathBuf,
    expected_sha256: String,
    compiled_fingerprint: String,
    file_identity: Identity,
    parent_identity: Identity,
    owner: u32,
    command_timeout: Duration,
    output_cap_bytes: usize,
}

impl Guard {
    pub(super) fn admit(
        path: PathBuf,
        expected: String,
        root: &Path,
        command_timeout: Duration,
        output_cap_bytes: usize,
    ) -> Result<Self, CollectorError> {
        if !path.is_absolute() || !is_sha256(&expected) {
            return Err(CollectorError::operational(
                "archive guard needs an absolute path and lowercase SHA-256",
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| CollectorError::operational("archive guard has no parent directory"))?;
        let name = path
            .file_name()
            .ok_or_else(|| CollectorError::operational("archive guard has no filename"))?;
        let physical_parent = fs::canonicalize(parent).map_err(CollectorError::operational)?;
        if physical_parent.join(name) != path || !is_real_directory(parent)? {
            return Err(CollectorError::operational(
                "archive guard path must be canonical in a real directory",
            ));
        }
        let parent_path = parent.to_path_buf();
        let metadata = real_file_metadata(&path)?;
        let parent_metadata = fs::symlink_metadata(parent).map_err(CollectorError::operational)?;
        let owner = rustix::process::geteuid().as_raw();
        validate_file_metadata(&metadata, owner)?;
        validate_parent_metadata(&parent_metadata, owner)?;
        if sha256_file(&path)? != expected {
            return Err(CollectorError::operational(
                "archive guard SHA-256 does not match supplied value",
            ));
        }
        let guard = Self {
            path,
            parent: parent_path,
            expected_sha256: expected,
            compiled_fingerprint: String::new(),
            file_identity: Identity::from(&metadata),
            parent_identity: Identity::from(&parent_metadata),
            owner,
            command_timeout,
            output_cap_bytes,
        };
        let compiled = guard.guard_output(&["--fingerprint"])?;
        if !is_sha256(&compiled) {
            return Err(CollectorError::operational(
                "archive guard compiled fingerprint is malformed",
            ));
        }
        let guard = Self {
            compiled_fingerprint: compiled,
            ..guard
        };
        guard.verify(root)?;
        Ok(guard)
    }

    pub(super) fn verify(&self, root: &Path) -> Result<(), CollectorError> {
        let parent_metadata =
            fs::symlink_metadata(&self.parent).map_err(CollectorError::operational)?;
        let metadata = real_file_metadata(&self.path)?;
        validate_file_metadata(&metadata, self.owner)?;
        validate_parent_metadata(&parent_metadata, self.owner)?;
        if Identity::from(&metadata) != self.file_identity
            || Identity::from(&parent_metadata) != self.parent_identity
            || sha256_file(&self.path)? != self.expected_sha256
        {
            return Err(CollectorError::operational(
                "archive guard changed during candidate execution",
            ));
        }
        let compiled = self.guard_output(&["--fingerprint"])?;
        let source = self.guard_output(&["source-fingerprint", path_text(root)?])?;
        if compiled != self.compiled_fingerprint || source != self.compiled_fingerprint {
            return Err(CollectorError::operational(
                "archive guard source fingerprint is stale or changed",
            ));
        }
        Ok(())
    }

    pub(super) fn run_candidate(
        &self,
        root: &Path,
        command: &mut Command,
        persist: impl FnOnce(&BoundedCommandOutput) -> Result<(), CollectorError>,
    ) -> Result<BoundedCommandOutput, CollectorError> {
        self.verify(root)?;
        let output = run_command_bounded(
            command,
            self.output_cap_bytes,
            self.command_timeout,
            super::COMMAND_CLEANUP_BUDGET,
        );
        let persisted = persist(&output).err();
        let verified = self.verify(root);
        let mut failures = Vec::new();
        if let Some(error) = &output.execution_error {
            failures.push(format!("execution {:?}: {error}", output.termination));
        }
        if let Some(error) = &output.cleanup_error {
            failures.push(format!("cleanup: {error}"));
        }
        if let Some(error) = persisted {
            failures.push(format!("persistence: {error}"));
        }
        if let Err(error) = verified {
            failures.push(format!("post-execution verification: {error}"));
        }
        if !failures.is_empty() {
            return Err(CollectorError::operational(format!(
                "candidate command failed: {}",
                failures.join("; ")
            )));
        }
        Ok(output)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn guard_output(&self, args: &[&str]) -> Result<String, CollectorError> {
        let mut command = Command::new(&self.path);
        command.args(args);
        let output = run_command_bounded(
            &mut command,
            self.output_cap_bytes,
            self.command_timeout,
            super::COMMAND_CLEANUP_BUDGET,
        );
        if output.execution_error.is_some() || output.cleanup_error.is_some() {
            return Err(CollectorError::operational(format!(
                "archive guard command failed: execution={:?} cleanup={:?}",
                output.execution_error, output.cleanup_error
            )));
        }
        if output.status != BoundedCommandStatus::Exited(0) {
            return Err(CollectorError::operational(format!(
                "archive guard command failed ({:?}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim_end()
            )));
        }
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| CollectorError::operational("archive guard output is not UTF-8"))?;
        let value = text.strip_suffix('\n').ok_or_else(|| {
            CollectorError::operational("archive guard output is not newline-terminated")
        })?;
        if value.contains('\n') || value.contains('\r') {
            return Err(CollectorError::operational(
                "archive guard output has extra lines",
            ));
        }
        Ok(value.to_owned())
    }
}

fn path_text(path: &Path) -> Result<&str, CollectorError> {
    path.to_str()
        .ok_or_else(|| CollectorError::operational("repository path is not UTF-8"))
}

fn real_file_metadata(path: &Path) -> Result<Metadata, CollectorError> {
    let metadata = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    if !metadata.file_type().is_file() {
        return Err(CollectorError::operational(
            "archive guard must be a regular non-symlink executable",
        ));
    }
    Ok(metadata)
}

fn is_real_directory(path: &Path) -> Result<bool, CollectorError> {
    let metadata = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    Ok(metadata.file_type().is_dir())
}

fn validate_file_metadata(metadata: &Metadata, owner: u32) -> Result<(), CollectorError> {
    let mode = metadata.permissions().mode();
    if metadata.uid() != owner || metadata.nlink() != 1 || mode & 0o022 != 0 || mode & 0o111 == 0 {
        return Err(CollectorError::operational(
            "archive guard ownership, permissions, or link count are unsafe",
        ));
    }
    Ok(())
}

fn validate_parent_metadata(metadata: &Metadata, owner: u32) -> Result<(), CollectorError> {
    if !metadata.file_type().is_dir()
        || metadata.uid() != owner
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CollectorError::operational(
            "archive guard parent must be private to the current user",
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, CollectorError> {
    let initial = real_file_metadata(path)?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(CollectorError::operational)?;
    let mut file = File::from(fd);
    let opened = file.metadata().map_err(CollectorError::operational)?;
    if !same_object(&initial, &opened) || !opened.file_type().is_file() {
        return Err(CollectorError::operational(
            "archive guard changed while hashing",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(CollectorError::operational)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if !same_object(
        &opened,
        &file.metadata().map_err(CollectorError::operational)?,
    ) || !same_object(
        &opened,
        &fs::symlink_metadata(path).map_err(CollectorError::operational)?,
    ) {
        return Err(CollectorError::operational(
            "archive guard changed while hashing",
        ));
    }
    Ok(filesystem::hex(&hasher.finalize()))
}

fn same_object(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino() && left.nlink() == right.nlink()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
