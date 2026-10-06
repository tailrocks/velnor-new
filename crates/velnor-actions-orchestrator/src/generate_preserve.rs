//! Preserve repository-owned `.github` content within the staged transaction.
//!
//! Workflows and declared generated formats are generator-owned; all other
//! entries are copied without following symbolic links. Unsupported file types
//! fail before the existing tree is replaced.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::apt_delivery::APT_DELIVERY_TREE_PATHS;
use crate::delivery_emit::oci_delivery::OCI_DELIVERY_TREE_PATHS;
use velnor_actions_contract::DECLARED_GITHUB_FORMATS;
use velnor_actions_contract::config::SwiftInputs;
use velnor_actions_contract::{MARKER_PREFIX, marker_for_version};
use velnor_actions_native::swift::{
    desktop_owned_paths, projected_profile_file, valid_profile_path,
};
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;

use crate::OrchestratorError;

#[path = "generate_retired_ownership.rs"]
mod retired;

/// Copy every repository-owned root entry, including hidden and empty trees.
pub(super) fn copy_repository_content(
    source: &Path,
    destination: &Path,
) -> Result<(), OrchestratorError> {
    if std::fs::symlink_metadata(source).is_ok_and(|metadata| metadata.is_symlink()) {
        return Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "symlink_refused".to_owned(),
        });
    }
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(io(source, err)),
    };
    std::fs::create_dir_all(destination).map_err(|err| io(destination, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| io(source, err))?;
        copy_entry(
            &entry.path(),
            &destination.join(entry.file_name()),
            Path::new(&entry.file_name()),
        )?;
    }
    Ok(())
}

/// Ownership derives from output inventories, including disabled release paths.
fn generator_owned(relative: &Path) -> bool {
    relative.starts_with("workflows")
        || DECLARED_GITHUB_FORMATS
            .iter()
            .map(|format| format.path)
            .chain(RELEASE_TREE_PATHS.iter().copied())
            .chain(APT_DELIVERY_TREE_PATHS.iter().copied())
            .chain(OCI_DELIVERY_TREE_PATHS.iter().copied())
            .chain(desktop_owned_paths())
            .any(|path| {
                Path::new(path)
                    .strip_prefix(".github")
                    .is_ok_and(|rel| rel == relative)
            })
}

/// Preserve files, directory permissions, and links as entries, never targets.
fn copy_entry(source: &Path, destination: &Path, relative: &Path) -> Result<(), OrchestratorError> {
    if generator_owned(relative) && !retired::is_retired_path(relative) {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(source).map_err(|err| io(source, err))?;
    if retired::is_retired_path(relative) && metadata.is_file() && retired::marked_owned(source)? {
        return Ok(());
    }
    if workload_profile_path(relative) && !metadata.is_dir() && owned_workload_profile(source)? {
        return Ok(());
    }
    if metadata.is_symlink() {
        copy_symlink(source, destination)?;
    } else if metadata.is_file() {
        std::fs::copy(source, destination).map_err(|err| io(source, err))?;
    } else if metadata.is_dir() {
        std::fs::create_dir(destination).map_err(|err| io(destination, err))?;
        for entry in std::fs::read_dir(source).map_err(|err| io(source, err))? {
            let entry = entry.map_err(|err| io(source, err))?;
            copy_entry(
                &entry.path(),
                &destination.join(entry.file_name()),
                &relative.join(entry.file_name()),
            )?;
        }
        std::fs::set_permissions(destination, metadata.permissions())
            .map_err(|err| io(destination, err))?;
    } else {
        return Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "unsupported_repository_entry".to_owned(),
        });
    }
    Ok(())
}

/// Only one JSON component in the compiled workload-profile namespace qualifies.
fn workload_profile_path(relative: &Path) -> bool {
    relative.parent() == Some(Path::new("velnor/desktop/workloads"))
        && relative
            .extension()
            .is_some_and(|extension| extension == "json")
}

/// A canonical marker from any generator version and a valid typed descriptor
/// establish ownership. Unmarked entries remain repository-owned; ambiguous
/// marked entries fail before replacement rather than being deleted or retained.
fn owned_workload_profile(source: &Path) -> Result<bool, OrchestratorError> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::NONBLOCK
        | rustix::fs::OFlags::CLOEXEC;
    let fd = rustix::fs::open(source, flags, rustix::fs::Mode::empty())
        .map_err(|error| io(source, std::io::Error::from(error)))?;
    let file = std::fs::File::from(fd);
    if !file
        .metadata()
        .map_err(|error| io(source, error))?
        .is_file()
    {
        return Err(profile_ownership_error(source, "not_a_regular_file"));
    }
    let mut reader = BufReader::new(file);
    let mut first = Vec::new();
    reader
        .by_ref()
        .take(512)
        .read_until(b'\n', &mut first)
        .map_err(|error| io(source, error))?;
    if !first.starts_with(MARKER_PREFIX.as_bytes()) {
        return Ok(false);
    }
    let first = std::str::from_utf8(&first)
        .map_err(|_| profile_ownership_error(source, "invalid_marker"))?;
    let line = first
        .strip_suffix('\n')
        .ok_or_else(|| profile_ownership_error(source, "invalid_marker"))?;
    let version = line
        .strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.split_once(';').map(|(version, _)| version))
        .ok_or_else(|| profile_ownership_error(source, "invalid_marker"))?;
    if !marker_for_version(version).is_ok_and(|marker| marker == line) {
        return Err(profile_ownership_error(source, "invalid_marker"));
    }
    let mut body = Vec::new();
    let remaining = crate::safe_read::MAX_REPO_FILE_BYTES
        .checked_sub(u64::try_from(first.len()).unwrap_or(u64::MAX))
        .ok_or_else(|| profile_ownership_error(source, "oversize"))?;
    reader
        .take(remaining + 1)
        .read_to_end(&mut body)
        .map_err(|error| io(source, error))?;
    if u64::try_from(body.len()).unwrap_or(u64::MAX) > remaining {
        return Err(profile_ownership_error(source, "oversize"));
    }
    validate_owned_profile(source, &body, version, first)?;
    Ok(true)
}

/// Strict schema validation and the owning adapter's exact path predicate.
fn validate_owned_profile(
    source: &Path,
    body: &[u8],
    version: &str,
    marker: &str,
) -> Result<(), OrchestratorError> {
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(body, body.len())
        .map_err(|_| profile_ownership_error(source, "invalid_profile"))?;
    let profile: SwiftInputs = serde_json::from_value(value)
        .map_err(|_| profile_ownership_error(source, "invalid_profile"))?;
    profile
        .validate(&source.display().to_string(), "native_desktop")
        .map_err(|_| profile_ownership_error(source, "invalid_profile"))?;
    let path = format!(
        ".github/velnor/desktop/workloads/{}",
        source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| profile_ownership_error(source, "invalid_profile_path"))?
    );
    if !valid_profile_path(&path) {
        return Err(profile_ownership_error(source, "invalid_profile_path"));
    }
    let canonical = projected_profile_file(&path, &profile, version)
        .map_err(|_| profile_ownership_error(source, "invalid_profile"))?;
    if canonical.source().as_bytes() != [marker.as_bytes(), body].concat() {
        return Err(profile_ownership_error(source, "noncanonical_profile"));
    }
    Ok(())
}

/// Explain why a marked profile could not safely establish generator ownership.
fn profile_ownership_error(source: &Path, problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!(
            "generated_profile_ownership_refused:{}:{problem}",
            source.display()
        ),
    }
}

/// Recreate a symbolic link with its literal target, including dangling links.
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), OrchestratorError> {
    let target = std::fs::read_link(source).map_err(|err| io(source, err))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, destination).map_err(|err| io(destination, err))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let metadata = std::fs::symlink_metadata(source).map_err(|err| io(source, err))?;
        let result = if metadata.file_type().is_symlink_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        };
        result.map_err(|err| io(destination, err))
    }
}

/// Retain the exact failing path in filesystem diagnostics.
fn io(path: &Path, error: std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

#[cfg(test)]
#[path = "generate_preserve_tests.rs"]
mod tests;
