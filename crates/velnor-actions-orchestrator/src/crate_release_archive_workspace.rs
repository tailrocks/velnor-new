// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ArchiveFiles, MAX_ARCHIVE};
use crate::{OrchestratorError, internal::internal};

pub(super) fn inherited_workspace_metadata(
    root: &std::path::Path,
    package_dir: &std::path::Path,
    files: &std::collections::BTreeSet<String>,
    archive: &ArchiveFiles,
    archive_path: &str,
) -> Result<Option<String>, OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    let parse = |relative: &std::path::Path| -> Result<toml::Value, OrchestratorError> {
        let name = relative.to_str().ok_or_else(invalid)?;
        if !files.contains(name) {
            return Err(invalid());
        }
        let bytes = match crate::safe_read::read_repo_bytes(root, name, MAX_ARCHIVE as u64)? {
            crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
            crate::safe_read::RepoBytes::Absent => return Err(invalid()),
        };
        toml::from_str(std::str::from_utf8(&bytes).map_err(|_| invalid())?).map_err(|_| invalid())
    };
    let member = parse(&package_dir.join("Cargo.toml"))?;
    let normalized: toml::Value = toml::from_str(
        std::str::from_utf8(&archive.get("Cargo.toml").ok_or_else(invalid)?.1)
            .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    let canonical_root = root.canonicalize().map_err(|_| invalid())?;
    let workspace = if let Some(relative) = member
        .get("package")
        .and_then(|p| p.get("workspace"))
        .and_then(toml::Value::as_str)
    {
        let absolute = root
            .join(package_dir)
            .join(relative)
            .canonicalize()
            .map_err(|_| invalid())?;
        let path = absolute
            .strip_prefix(&canonical_root)
            .map_err(|_| invalid())?
            .to_path_buf();
        let manifest = parse(&path.join("Cargo.toml"))?;
        if manifest.get("workspace").is_none_or(|v| !v.is_table()) {
            return Err(invalid());
        }
        Some((path, manifest))
    } else {
        let mut found = None;
        for ancestor in package_dir.ancestors() {
            let candidate = ancestor.join("Cargo.toml");
            if candidate.to_str().is_some_and(|name| files.contains(name)) {
                let manifest = parse(&candidate)?;
                if manifest.get("workspace").is_some_and(toml::Value::is_table) {
                    found = Some((ancestor.to_path_buf(), manifest));
                    break;
                }
            }
        }
        found
    };
    let Some((workspace_dir, workspace)) = workspace else {
        return Ok(None);
    };
    for key in ["license-file", "readme"] {
        if member
            .get("package")
            .and_then(|p| p.get(key))
            .and_then(|v| v.get("workspace"))
            .and_then(toml::Value::as_bool)
            != Some(true)
            || normalized
                .get("package")
                .and_then(|p| p.get(key))
                .and_then(toml::Value::as_str)
                != Some(archive_path)
        {
            continue;
        }
        let declared = workspace
            .get("workspace")
            .and_then(|v| v.get("package"))
            .and_then(|v| v.get(key))
            .and_then(toml::Value::as_str)
            .ok_or_else(invalid)?;
        let declared_path = std::path::Path::new(declared);
        if declared_path.is_absolute()
            || declared_path.file_name().and_then(std::ffi::OsStr::to_str) != Some(archive_path)
        {
            return Err(invalid());
        }
        let absolute = root
            .join(&workspace_dir)
            .join(declared_path)
            .canonicalize()
            .map_err(|_| invalid())?;
        let relative = absolute
            .strip_prefix(&canonical_root)
            .map_err(|_| invalid())?
            .to_str()
            .ok_or_else(invalid)?
            .to_owned();
        if !files.contains(&relative) {
            return Err(invalid());
        }
        return Ok(Some(relative));
    }
    Ok(None)
}
