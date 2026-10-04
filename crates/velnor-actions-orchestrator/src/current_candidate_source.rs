//! Bind frozen checkout bytes to the authenticated current candidate service tree.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::path::Path;

use rustix::fs::{Mode, OFlags, open, openat};
use velnor_actions_contract::{normalize_posix_path, parse_strict_json};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BLOB_BYTES: u64 = velnor_actions_mise::command::OUTPUT_CAPTURE_LIMIT_BYTES as u64;

#[derive(Debug)]
struct SourceBlob {
    sha: String,
    size: u64,
    executable: bool,
}

/// Only the original root supplies invocation cwd; local Git objects grant nothing.
pub(crate) fn verify(
    root: &Path,
    frozen: &Path,
    repository: &str,
    candidate: &str,
) -> Result<(), OrchestratorError> {
    validate_sha(candidate)?;
    let repository = crate::origin::validate_repository_slug(repository)
        .ok_or_else(|| failed("repository_invalid"))?;
    let catalog = ToolCatalog::pinned();
    verify_with(frozen, &repository, candidate, |endpoint, raw| {
        api(&catalog, root, endpoint, raw)
    })
}

/// Transport injection exercises the complete proof without network execution.
fn verify_with(
    frozen: &Path,
    repository: &str,
    candidate: &str,
    mut read: impl FnMut(&str, bool) -> Result<Vec<u8>, OrchestratorError>,
) -> Result<(), OrchestratorError> {
    validate_sha(candidate)?;
    let commit = read(
        &format!("repos/{repository}/git/commits/{candidate}"),
        false,
    )?;
    let tree_sha = commit_tree(&commit, candidate)?;
    let tree = read(
        &format!("repos/{repository}/git/trees/{tree_sha}?recursive=1"),
        false,
    )?;
    let blobs = parse_tree(&tree, &tree_sha)?;
    compare_source(frozen, &blobs, |sha| {
        read(&format!("repos/{repository}/git/blobs/{sha}"), true)
    })
}

fn commit_tree(bytes: &[u8], candidate: &str) -> Result<String, OrchestratorError> {
    let value = strict_json(bytes)?;
    if value["sha"] != candidate {
        return Err(failed("commit_mismatch"));
    }
    let tree = value["tree"]["sha"]
        .as_str()
        .ok_or_else(|| failed("commit_tree_missing"))?;
    validate_sha(tree)?;
    Ok(tree.to_owned())
}

fn parse_tree(
    bytes: &[u8],
    expected_sha: &str,
) -> Result<BTreeMap<String, SourceBlob>, OrchestratorError> {
    let value = strict_json(bytes)?;
    if value["sha"] != expected_sha || value["truncated"] != false {
        return Err(failed("tree_mismatch_or_truncated"));
    }
    let entries = value["tree"]
        .as_array()
        .filter(|entries| entries.len() <= 20_000)
        .ok_or_else(|| failed("tree_inventory_invalid"))?;
    let mut paths = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut blobs = BTreeMap::new();
    let mut total_size = 0_u64;
    for entry in entries {
        let path = entry["path"]
            .as_str()
            .ok_or_else(|| failed("tree_path_missing"))?;
        validate_path(path)?;
        if !paths.insert(path.to_owned()) {
            return Err(failed("tree_path_duplicate"));
        }
        let sha = entry["sha"]
            .as_str()
            .ok_or_else(|| failed("tree_object_missing"))?;
        validate_sha(sha)?;
        match (entry["type"].as_str(), entry["mode"].as_str()) {
            (Some("tree"), Some("040000")) => {
                directories.insert(path.to_owned());
            }
            (Some("blob"), Some(mode @ ("100644" | "100755"))) => {
                let size = entry["size"]
                    .as_u64()
                    .filter(|size| *size <= MAX_BLOB_BYTES)
                    .ok_or_else(|| failed("tree_blob_size_invalid"))?;
                total_size = total_size
                    .checked_add(size)
                    .filter(|total| *total <= MAX_SOURCE_BYTES)
                    .ok_or_else(|| failed("tree_aggregate_size_limit"))?;
                blobs.insert(
                    path.to_owned(),
                    SourceBlob {
                        sha: sha.to_owned(),
                        size,
                        executable: mode == "100755",
                    },
                );
            }
            _ => return Err(failed("tree_nonregular_object")),
        }
    }
    validate_tree_topology(&paths, &directories, &blobs)?;
    Ok(blobs)
}

fn validate_tree_topology(
    paths: &BTreeSet<String>,
    directories: &BTreeSet<String>,
    blobs: &BTreeMap<String, SourceBlob>,
) -> Result<(), OrchestratorError> {
    for path in paths {
        if let Some((parent, _)) = path.rsplit_once('/')
            && !directories.contains(parent)
        {
            return Err(failed("tree_parent_missing"));
        }
    }
    for directory in directories {
        let prefix = format!("{directory}/");
        if !blobs.keys().any(|path| path.starts_with(&prefix)) {
            return Err(failed("tree_empty_directory"));
        }
    }
    Ok(())
}

fn compare_source(
    frozen: &Path,
    blobs: &BTreeMap<String, SourceBlob>,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, OrchestratorError>,
) -> Result<(), OrchestratorError> {
    if !cfg!(unix) {
        return Err(failed("checkout_executable_mode_unavailable"));
    }
    let actual = inventory(frozen)?;
    if actual.iter().ne(blobs.keys()) {
        return Err(failed("checkout_paths_mismatch"));
    }
    let root = open(frozen, directory_flags(), Mode::empty())
        .map_err(|_| failed("checkout_root_unreadable"))?;
    for (path, blob) in blobs {
        let (bytes, executable) = read_file(&root, path, blob.size)?;
        let service_bytes = read(&blob.sha)?;
        if executable != blob.executable
            || service_bytes.len() as u64 != blob.size
            || bytes != service_bytes
        {
            return Err(failed(format!("checkout_blob_mismatch:{path}")));
        }
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<BTreeSet<String>, OrchestratorError> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = BTreeSet::new();
    let mut count = 0;
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|_| failed("checkout_directory_unreadable"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(failed("checkout_directory_nonregular"));
        }
        for entry in fs::read_dir(directory).map_err(|_| failed("checkout_inventory_unreadable"))? {
            let entry = entry.map_err(|_| failed("checkout_inventory_unreadable"))?;
            count += 1;
            if count > 20_000 {
                return Err(failed("checkout_inventory_limit"));
            }
            let full = entry.path();
            let path = full
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| failed("checkout_path_invalid"))?;
            validate_path(path)?;
            let kind = entry
                .file_type()
                .map_err(|_| failed("checkout_kind_unreadable"))?;
            if kind.is_dir() {
                pending.push(full);
            } else if kind.is_file() {
                paths.insert(path.to_owned());
            } else {
                return Err(failed("checkout_nonregular_input"));
            }
        }
    }
    Ok(paths)
}

/// Open every ancestor without following links, then bind bytes and mode together.
fn read_file(
    root: &rustix::fd::OwnedFd,
    path: &str,
    expected_size: u64,
) -> Result<(Vec<u8>, bool), OrchestratorError> {
    let mut directory = rustix::io::dup(root).map_err(|_| failed("checkout_root_handle"))?;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_some() {
            directory = openat(&directory, component, directory_flags(), Mode::empty())
                .map_err(|_| failed("checkout_parent_unreadable"))?;
            continue;
        }
        let fd = openat(
            &directory,
            component,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|_| failed("checkout_file_unreadable"))?;
        let file = fs::File::from(fd);
        let metadata = file
            .metadata()
            .map_err(|_| failed("checkout_metadata_unreadable"))?;
        if !metadata.is_file() {
            return Err(failed("checkout_nonregular_input"));
        }
        if metadata.len() != expected_size {
            return Err(failed("checkout_blob_size_mismatch"));
        }
        let bytes = bounded_bytes(file, expected_size)?;
        return Ok((bytes, executable(&metadata)));
    }
    Err(failed("checkout_empty_path"))
}

/// One extra byte detects growth after metadata admission without unbounded reads.
fn bounded_bytes(
    reader: impl std::io::Read,
    expected_size: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    if expected_size > MAX_BLOB_BYTES {
        return Err(failed("checkout_blob_size_limit"));
    }
    let mut bytes = Vec::new();
    reader
        .take(expected_size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed("checkout_bytes_unreadable"))?;
    if bytes.len() as u64 != expected_size {
        return Err(failed("checkout_blob_size_mismatch"));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o100 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn validate_path(path: &str) -> Result<(), OrchestratorError> {
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path.contains('\\')
        || path.split('/').any(|part| {
            part.is_empty() || matches!(part, "." | "..") || part.eq_ignore_ascii_case(".git")
        })
        || !normalize_posix_path(path).is_ok_and(|normalized| normalized == path)
    {
        return Err(failed("tree_path_noncanonical"));
    }
    Ok(())
}

fn validate_sha(sha: &str) -> Result<(), OrchestratorError> {
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || sha.bytes().all(|byte| byte == b'0')
    {
        return Err(failed("object_sha_invalid"));
    }
    Ok(())
}

fn strict_json(bytes: &[u8]) -> Result<serde_json::Value, OrchestratorError> {
    let text = std::str::from_utf8(bytes).map_err(|_| failed("service_json_non_utf8"))?;
    parse_strict_json(text).map_err(|_| failed("service_json_invalid"))
}

fn api(
    _catalog: &ToolCatalog,
    _root: &Path,
    _endpoint: &str,
    _raw: bool,
) -> Result<Vec<u8>, OrchestratorError> {
    // Baseline commands still inherit HOME/config and transport overrides.
    // Authentication requires a pinned request with cleared service environment.
    Err(failed("candidate_source_authenticated_transport_pending"))
}

fn failed(problem: impl Into<String>) -> OrchestratorError {
    crate::internal::internal(&format!(
        "helper_current_candidate_source:{}",
        problem.into()
    ))
}

#[cfg(test)]
#[path = "current_candidate_source_tests.rs"]
mod tests;
