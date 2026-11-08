// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use flate2::bufread::GzDecoder;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

const MAX_ARCHIVE: usize = 64 * 1024 * 1024;
const MAX_UNPACKED: u64 = 256 * 1024 * 1024;
const MAX_CARGO_GENERATED_ENTRIES: usize = 4;
const MAX_ENTRIES: usize =
    crate::release::crate_release_source::MAX_SOURCE_ENTRIES + MAX_CARGO_GENERATED_ENTRIES;

pub(crate) type ArchiveFiles = BTreeMap<String, (u32, Vec<u8>)>;

pub fn verify_archive_tracked_content(
    archive: &[u8],
    root: &std::path::Path,
    source_sha: &str,
    manifest: &str,
    package: &str,
    version: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    verify_archive_tracked_content_inner(
        archive, root, source_sha, manifest, package, version, git, false,
    )
}

pub(crate) fn verify_reconstructed_archive_tracked_content(
    archive: &[u8],
    root: &std::path::Path,
    source_sha: &str,
    manifest: &str,
    package: &str,
    version: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    verify_archive_tracked_content_inner(
        archive, root, source_sha, manifest, package, version, git, true,
    )
}

fn verify_archive_tracked_content_inner(
    archive: &[u8],
    root: &std::path::Path,
    source_sha: &str,
    manifest: &str,
    package: &str,
    version: &str,
    git: &crate::crate_release_git::ReleaseGit,
    generated_lock: bool,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    let package_dir = source_package_dir(manifest, package, version)?;
    crate::verify_crate_release_source(root, source_sha, git)?;
    let files = tracked_paths(root, git)?;
    let archive = read_archive(archive, &format!("{package}-{version}/"))?;
    validate_archive_generated(
        &archive,
        root,
        source_sha,
        manifest,
        package_dir,
        package,
        version,
    )?;
    for (path, (mode, contents)) in &archive {
        if matches!(
            path.as_str(),
            "Cargo.toml" | "Cargo.toml.orig" | ".cargo_vcs_info.json"
        ) {
            continue;
        }
        let local = package_dir.join(path);
        let local = local.to_str().ok_or_else(invalid)?;
        let relative = if files.contains(local) {
            local.to_owned()
        } else if generated_lock && path == "Cargo.lock" && *mode == 0 {
            continue;
        } else {
            inherited_workspace_metadata(root, package_dir, &files, &archive, path)?
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
        let metadata =
            std::fs::symlink_metadata(root.join(source_relative)).map_err(|_| invalid())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if (metadata.permissions().mode() & 0o111 != 0) != (*mode != 0) {
                return Err(invalid());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (metadata, mode);
            return Err(invalid());
        }
        if source != *contents {
            return Err(invalid());
        }
    }
    crate::verify_crate_release_source(root, source_sha, git)
}

fn inherited_workspace_metadata(
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

fn source_package_dir<'a>(
    manifest: &'a str,
    package: &str,
    version: &str,
) -> Result<&'a std::path::Path, OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    if !velnor_actions_contract::config::is_cargo_version(version)
        || package.is_empty()
        || !package
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(invalid());
    }
    let path = std::path::Path::new(manifest);
    if path.file_name().and_then(std::ffi::OsStr::to_str) != Some("Cargo.toml")
        || !path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Err(invalid());
    }
    path.parent().ok_or_else(invalid)
}

fn tracked_paths(
    root: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<std::collections::BTreeSet<String>, OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    let tracked = velnor_actions_mise::GitRequest::ls_files(vec!["--stage".into(), "-z".into()])
        .run_release_in(root, git)
        .map_err(|_| invalid())?;
    if !tracked.success {
        return Err(invalid());
    }
    let mut files = std::collections::BTreeSet::new();
    for item in tracked
        .stdout
        .split(|b| *b == 0)
        .filter(|item| !item.is_empty())
    {
        let item = std::str::from_utf8(item).map_err(|_| invalid())?;
        let (header, path) = item.split_once('\t').ok_or_else(invalid)?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3 || fields[2] != "0" || !files.insert(path.to_owned()) {
            return Err(invalid());
        }
    }
    Ok(files)
}

fn validate_archive_generated(
    archive: &BTreeMap<String, (u32, Vec<u8>)>,
    root: &std::path::Path,
    source_sha: &str,
    manifest: &str,
    package_dir: &std::path::Path,
    package: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_archive_source_evidence_invalid");
    let source = match crate::safe_read::read_repo_bytes(root, manifest, MAX_ARCHIVE as u64)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => return Err(invalid()),
    };
    if archive.get("Cargo.toml.orig").map(|entry| &entry.1) != Some(&source) {
        return Err(invalid());
    }
    let normalized = &archive.get("Cargo.toml").ok_or_else(invalid)?.1;
    let parsed: toml::Value =
        toml::from_str(std::str::from_utf8(normalized).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if parsed["package"]["name"].as_str() != Some(package)
        || parsed["package"]["version"].as_str() != Some(version)
    {
        return Err(invalid());
    }
    let vcs = &archive.get(".cargo_vcs_info.json").ok_or_else(invalid)?.1;
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(vcs, 1024 * 1024)
        .map_err(|_| invalid())?;
    if value.as_object().is_none_or(|object| object.len() != 2)
        || value["git"]
            .as_object()
            .is_none_or(|object| object.len() != 1)
        || value["git"]["sha1"].as_str() != Some(source_sha)
        || value["path_in_vcs"].as_str() != package_dir.to_str()
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn verify_crate_archive(
    expected: &[u8],
    published: &[u8],
    package: &str,
    version: &str,
    checksum: &str,
) -> Result<(), OrchestratorError> {
    if checksum.len() != 64
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || crate::planner::generator::sha256_hex(published) != checksum
    {
        return Err(internal("crate_registry_checksum_mismatch"));
    }
    let prefix = format!("{package}-{version}/");
    let expected_files = read_archive(expected, &prefix)?;
    let actual_files = read_archive(published, &prefix)?;
    if expected_files != actual_files {
        return Err(internal("crate_archive_content_mismatch"));
    }
    Ok(())
}

pub(crate) fn read_archive(bytes: &[u8], prefix: &str) -> Result<ArchiveFiles, OrchestratorError> {
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE {
        return Err(internal("crate_archive_size_invalid"));
    }
    let mut decoder = GzDecoder::new(Cursor::new(bytes));
    let mut uncompressed = Vec::new();
    decoder
        .by_ref()
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut uncompressed)
        .map_err(|_| internal("crate_archive_gzip_invalid"))?;
    if uncompressed.len() as u64 > MAX_UNPACKED
        || decoder.into_inner().position() != bytes.len() as u64
    {
        return Err(internal("crate_archive_gzip_trailing_or_large"));
    }
    let mut archive = tar::Archive::new(Cursor::new(&uncompressed));
    let entries = archive
        .entries()
        .map_err(|_| internal("crate_archive_tar_invalid"))?;
    let mut files = BTreeMap::new();
    let mut last_end = 0_u64;
    for result in entries {
        let mut entry = result.map_err(|_| internal("crate_archive_entry_invalid"))?;
        if files.len() >= MAX_ENTRIES || !entry.header().entry_type().is_file() {
            return Err(internal("crate_archive_entry_type_or_count"));
        }
        let path = entry
            .path()
            .map_err(|_| internal("crate_archive_path_invalid"))?
            .into_owned();
        let name = path
            .to_str()
            .ok_or_else(|| internal("crate_archive_path_invalid"))?;
        let relative = name
            .strip_prefix(prefix)
            .ok_or_else(|| internal("crate_archive_prefix_mismatch"))?;
        if relative.is_empty()
            || relative.contains('\\')
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(internal("crate_archive_path_invalid"));
        }
        let size = entry.size();
        last_end = entry
            .raw_file_position()
            .checked_add(
                size.div_ceil(512)
                    .checked_mul(512)
                    .ok_or_else(|| internal("crate_archive_size_invalid"))?,
            )
            .ok_or_else(|| internal("crate_archive_size_invalid"))?;
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|_| internal("crate_archive_read_failed"))?;
        let mode = entry
            .header()
            .mode()
            .map_err(|_| internal("crate_archive_mode_invalid"))?
            & 0o111;
        if content.len() as u64 != size
            || files.insert(relative.to_owned(), (mode, content)).is_some()
        {
            return Err(internal("crate_archive_duplicate_or_size"));
        }
    }
    if files.len() > crate::release::crate_release_source::MAX_SOURCE_ENTRIES + 3
        && !files.contains_key("Cargo.lock")
    {
        return Err(internal("crate_archive_entry_type_or_count"));
    }
    let end = usize::try_from(last_end).map_err(|_| internal("crate_archive_size_invalid"))?;
    if files.is_empty()
        || end
            .checked_add(1024)
            .is_none_or(|minimum| minimum > uncompressed.len())
        || uncompressed[end..].iter().any(|byte| *byte != 0)
    {
        return Err(internal("crate_archive_trailing_invalid"));
    }
    Ok(files)
}

pub(crate) fn crate_archive_features(
    bytes: &[u8],
    package: &str,
    version: &str,
) -> Result<BTreeMap<String, Vec<String>>, OrchestratorError> {
    let files = read_archive(bytes, &format!("{package}-{version}/"))?;
    let manifest = files
        .get("Cargo.toml")
        .ok_or_else(|| internal("crate_archive_manifest_missing"))?;
    let text =
        std::str::from_utf8(&manifest.1).map_err(|_| internal("crate_archive_manifest_invalid"))?;
    let value: toml::Value =
        toml::from_str(text).map_err(|_| internal("crate_archive_manifest_invalid"))?;
    let mut features = BTreeMap::new();
    if let Some(feature_table) = value.get("features") {
        let table = feature_table
            .as_table()
            .ok_or_else(|| internal("crate_archive_features_invalid"))?;
        for (name, members) in table {
            let array = members
                .as_array()
                .ok_or_else(|| internal("crate_archive_features_invalid"))?;
            let mut entries = array
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| internal("crate_archive_features_invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort();
            features.insert(name.clone(), entries);
        }
    }
    Ok(features)
}

pub(crate) fn crate_archive_inventory(
    bytes: &[u8],
    package: &str,
    version: &str,
) -> Result<BTreeMap<String, (u32, String)>, OrchestratorError> {
    Ok(read_archive(bytes, &format!("{package}-{version}/"))?
        .into_iter()
        .map(|(path, (mode, content))| {
            (
                path,
                (mode, crate::planner::generator::sha256_hex(&content)),
            )
        })
        .collect())
}
