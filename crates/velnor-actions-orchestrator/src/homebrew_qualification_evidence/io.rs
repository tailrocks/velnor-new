// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{io::Read, os::unix::fs::MetadataExt, path::Path};

pub(super) fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    let file = std::fs::File::from(
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("homebrew_evidence_file"))?,
    );
    let metadata = file
        .metadata()
        .map_err(|_| internal("homebrew_evidence_file"))?;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > limit
        || metadata.nlink() != 1
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(internal("homebrew_evidence_file"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| internal("homebrew_evidence_read"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(internal("homebrew_evidence_size"));
    }
    Ok(bytes)
}

pub(super) fn require_private_directory(path: &Path) -> Result<(), OrchestratorError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| internal("homebrew_evidence_directory"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(internal("homebrew_evidence_directory"));
    }
    Ok(())
}
