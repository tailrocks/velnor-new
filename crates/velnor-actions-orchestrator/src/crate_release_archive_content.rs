// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ArchiveFiles, MAX_ARCHIVE, OrchestratorError, inherited_workspace_metadata, internal};

pub(super) fn verify_archive_file(
    root: &std::path::Path,
    package_dir: &std::path::Path,
    files: &std::collections::BTreeSet<String>,
    archive: &ArchiveFiles,
    path: &str,
    entry: (u32, &[u8]),
    generated_lock: bool,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    let (mode, contents) = entry;
    let local = package_dir.join(path);
    let local = local.to_str().ok_or_else(invalid)?;
    let relative = if files.contains(local) {
        local.to_owned()
    } else if generated_lock && path == "Cargo.lock" && mode == 0 {
        return Ok(());
    } else {
        inherited_workspace_metadata(root, package_dir, files, archive, path)?
            .ok_or_else(invalid)?
    };
    let path = root.join(&relative);
    let source_relative = if std::fs::symlink_metadata(&path)
        .map_err(|_| invalid())?
        .file_type()
        .is_symlink()
    {
        let canonical_root = root.canonicalize().map_err(|_| invalid())?;
        let target = path.canonicalize().map_err(|_| invalid())?;
        let target = target
            .strip_prefix(canonical_root)
            .map_err(|_| invalid())?
            .to_str()
            .ok_or_else(invalid)?;
        if !files.contains(target) {
            return Err(invalid());
        }
        target.to_owned()
    } else {
        relative
    };
    let source =
        match crate::safe_read::read_repo_bytes(root, &source_relative, MAX_ARCHIVE as u64)? {
            crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
            crate::safe_read::RepoBytes::Absent => return Err(invalid()),
        };
    let metadata = std::fs::symlink_metadata(root.join(source_relative)).map_err(|_| invalid())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if (metadata.permissions().mode() & 0o111 != 0) != (mode != 0) {
            return Err(invalid());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, mode);
        return Err(invalid());
    }
    if source != contents {
        return Err(invalid());
    }
    Ok(())
}
