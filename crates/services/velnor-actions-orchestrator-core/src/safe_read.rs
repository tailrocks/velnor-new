//! Symlink-rejecting, root-constrained, size-capped file reads (X6).
//!
//! Config, manifest, event-payload, and artifact-output reads share bounded
//! helpers here. Small plan-time reads canonicalize and constrain paths under
//! the repository root, reject symlinks, and stop one byte past the bound.
//! Absent files report [`RepoRead::Absent`]; every other failure is an error,
//! never silently masked as absent.
//!
//! Small plan-time readers pin the final regular-file handle but retain a
//! same-user parent-component replacement residual during pathname resolution.
//! The runtime [`stream_repo_file`] path uses a descriptor-relative walk, keeps
//! opened directories alive, and opens the leaf with `O_NOFOLLOW|O_NONBLOCK`
//! before checking that it is a regular file.

use std::fs;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::Path;

use velnor_actions_tofu_core::{FileCache, PinnedOutcome};

use crate::OrchestratorError;

/// Maximum bytes read from one repo or event file (8 MiB).
///
/// Matches the contract's untrusted-document bound: legitimate config,
/// manifest, and event-payload documents are kilobytes.
pub const MAX_REPO_FILE_BYTES: u64 = velnor_actions_contract_config::MAX_CHECK_SOURCE_BYTES as u64;

/// Outcome of a root-constrained repo file read.
#[derive(Debug)]
pub enum RepoRead {
    /// The file is absent.
    Absent,
    /// The file read within the bound.
    Text(String),
}

/// Outcome of reading bounded root-contained repository bytes.
#[derive(Debug)]
pub enum RepoBytes {
    /// The file is absent.
    Absent,
    /// The file read within the bound without an encoding assumption.
    Bytes(Vec<u8>),
}

/// Read one repo-relative file: absent, or text within `max_bytes`.
///
/// Rejects symlinks, non-files, root escapes, oversize content, and
/// invalid UTF-8 with typed errors; only absence returns [`RepoRead::Absent`].
pub fn read_repo_file(
    root: &Path,
    rel: &str,
    max_bytes: u64,
) -> Result<RepoRead, OrchestratorError> {
    read_repo_file_until_inner(root, rel, max_bytes, None)
}

/// Read a root-contained text file while checking a shared check deadline.
pub fn read_repo_file_until(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<RepoRead, OrchestratorError> {
    read_repo_file_until_inner(root, rel, max_bytes, Some(deadline))
}

fn read_repo_file_until_inner(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<RepoRead, OrchestratorError> {
    match read_repo_bytes_until_inner(root, rel, max_bytes, deadline)? {
        RepoBytes::Absent => Ok(RepoRead::Absent),
        RepoBytes::Bytes(bytes) => String::from_utf8(bytes)
            .map(RepoRead::Text)
            .map_err(|error| unreadable(&root.join(rel), error.to_string())),
    }
}

/// Read bounded repository bytes without requiring UTF-8 text.
///
/// Preserves the text reader's root containment, symlink rejection, regular-file
/// requirement and handle-pinned size bound.
pub fn read_repo_bytes(
    root: &Path,
    rel: &str,
    max_bytes: u64,
) -> Result<RepoBytes, OrchestratorError> {
    read_repo_bytes_until_inner(root, rel, max_bytes, None)
}

/// Read root-contained bytes while checking a shared named-check deadline.
pub fn read_repo_bytes_until(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<RepoBytes, OrchestratorError> {
    read_repo_bytes_until_inner(root, rel, max_bytes, Some(deadline))
}

fn read_repo_bytes_until_inner(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<RepoBytes, OrchestratorError> {
    let path = root.join(rel);
    match fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(RepoBytes::Absent),
        Err(err) => return Err(unreadable(&path, err.to_string())),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(unsafe_path(&path, "symlink_refused"));
        }
        Ok(meta) if !meta.is_file() => return Err(unreadable(&path, "not_a_file")),
        Ok(_) => {}
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|err| unreadable(root, err.to_string()))?;
    let canonical = path
        .canonicalize()
        .map_err(|err| unreadable(&path, err.to_string()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(unsafe_path(&path, "root_escape"));
    }
    Ok(RepoBytes::Bytes(read_capped_bytes_until(
        &canonical, max_bytes, deadline,
    )?))
}

/// Read one repo-relative file through the shared pinned compartment.
///
/// Hits return the first read's outcome; misses run [`read_repo_file`]
/// and store its outcome (text, absence, or the error's display
/// string). Callers map the plain outcome onto their own handling;
/// the secure checks always run on the miss path.
pub fn read_repo_file_cached(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    reads: &mut FileCache,
) -> PinnedOutcome {
    let key = root.join(rel);
    if let Some(hit) = reads.pinned(&key) {
        return hit;
    }
    let outcome = match read_repo_file(root, rel, max_bytes) {
        Ok(RepoRead::Text(text)) => PinnedOutcome::Text(text),
        Ok(RepoRead::Absent) => PinnedOutcome::Absent,
        Err(err) => PinnedOutcome::Unreadable(err.to_string()),
    };
    reads.store_pinned(key, outcome.clone());
    outcome
}

/// Read one event-payload file outside the checkout, within `max_bytes`.
///
/// The payload lives outside the repository by design, so no root
/// constraint applies; symlinks, non-files, oversize content, and
/// invalid UTF-8 still fail closed as unreadable payloads.
pub fn read_event_file(path: &Path, max_bytes: u64) -> Result<String, OrchestratorError> {
    use crate::error::internal;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(internal("unreadable_event_payload"));
        }
        Ok(meta) if !meta.is_file() => return Err(internal("unreadable_event_payload")),
        Err(_) | Ok(_) => {}
    }
    read_capped(path, max_bytes).map_err(|_| internal("unreadable_event_payload"))
}

/// Read one file through a pinned handle, stopping past `max_bytes`.
///
/// Opens with `O_NOFOLLOW`, requires the open handle to `fstat` as a
/// regular file, and reads only through that handle. Open/fstat
/// failures keep the historical [`std::io::Error`] strings; a symlink
/// swapped in after the pre-checks refuses exactly like a pre-check
/// symlink.
fn read_capped(path: &Path, max_bytes: u64) -> Result<String, OrchestratorError> {
    let bytes = read_capped_bytes(path, max_bytes)?;
    String::from_utf8(bytes).map_err(|err| unreadable(path, err.to_string()))
}

fn read_capped_bytes(path: &Path, max_bytes: u64) -> Result<Vec<u8>, OrchestratorError> {
    read_capped_bytes_until(path, max_bytes, None)
}

fn read_capped_bytes_until(
    path: &Path,
    max_bytes: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut bytes = Vec::new();
    stream_capped_file(path, max_bytes, deadline, &mut |chunk| {
        bytes.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(bytes)
}

fn stream_capped_file(
    path: &Path,
    max_bytes: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
    on_chunk: &mut impl FnMut(&[u8]) -> Result<(), OrchestratorError>,
) -> Result<u64, OrchestratorError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| {
        if err == rustix::io::Errno::LOOP {
            unsafe_path(path, "symlink_refused")
        } else {
            unreadable(path, std::io::Error::from(err).to_string())
        }
    })?;
    stream_open_file(fd, path, max_bytes, deadline, on_chunk)
}

fn stream_open_file(
    fd: OwnedFd,
    path: &Path,
    max_bytes: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
    on_chunk: &mut impl FnMut(&[u8]) -> Result<(), OrchestratorError>,
) -> Result<u64, OrchestratorError> {
    let filetype = rustix::fs::fstat(&fd)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|err| unreadable(path, std::io::Error::from(err).to_string()))?;
    if filetype != rustix::fs::FileType::RegularFile {
        return Err(unreadable(path, "not_a_file"));
    }
    let mut file = fs::File::from(fd);
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut total = 0_u64;
    loop {
        check_deadline(deadline)?;
        let remaining = max_bytes.saturating_add(1).saturating_sub(total);
        if remaining == 0 {
            return Err(unreadable(path, "oversize"));
        }
        let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let count = file
            .read(&mut buffer[..limit])
            .map_err(|err| unreadable(path, err.to_string()))?;
        check_deadline(deadline)?;
        if count == 0 {
            break;
        }
        let count_u64 = u64::try_from(count).unwrap_or(u64::MAX);
        let next = total.saturating_add(count_u64);
        if next > max_bytes {
            let allowed = usize::try_from(max_bytes.saturating_sub(total)).unwrap_or(usize::MAX);
            if allowed > 0 {
                on_chunk(&buffer[..allowed])?;
            }
            return Err(unreadable(path, "oversize"));
        }
        on_chunk(&buffer[..count])?;
        total = next;
    }
    Ok(total)
}

fn check_deadline(
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| crate::error::internal(&error.to_string()))?;
    }
    Ok(())
}

/// Build an IO error for one path.
pub(super) fn unreadable(path: &Path, problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), problem)
}

/// Build an unsafe-path error for one path.
pub(super) fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
mod stream;
pub use stream::stream_repo_file;

#[cfg(test)]
mod tests;
