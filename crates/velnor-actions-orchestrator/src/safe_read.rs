use std::fs;
use std::io::Read;
use std::path::Path;

use velnor_actions_tofu::{FileCache, PinnedOutcome};

use crate::OrchestratorError;

pub(crate) const MAX_REPO_FILE_BYTES: u64 = velnor_actions_contract::MAX_CHECK_SOURCE_BYTES as u64;

#[derive(Debug)]
pub(crate) enum RepoRead {
    Absent,
    Text(String),
}

#[derive(Debug)]
pub(crate) enum RepoBytes {
    Absent,
    Bytes(Vec<u8>),
}

pub(crate) fn read_repo_file(
    root: &Path,
    rel: &str,
    max_bytes: u64,
) -> Result<RepoRead, OrchestratorError> {
    read_repo_file_until_inner(root, rel, max_bytes, None)
}

pub(crate) fn read_repo_file_until(
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

pub(crate) fn read_repo_bytes(
    root: &Path,
    rel: &str,
    max_bytes: u64,
) -> Result<RepoBytes, OrchestratorError> {
    read_repo_bytes_until_inner(root, rel, max_bytes, None)
}

pub(crate) fn read_repo_bytes_until(
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

pub(crate) fn read_repo_file_cached(
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

pub(crate) fn read_event_file(path: &Path, max_bytes: u64) -> Result<String, OrchestratorError> {
    use crate::internal::internal;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(internal("unreadable_event_payload"));
        }
        Ok(meta) if !meta.is_file() => return Err(internal("unreadable_event_payload")),
        Err(_) | Ok(_) => {}
    }
    read_capped(path, max_bytes).map_err(|_| internal("unreadable_event_payload"))
}

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
    let filetype = rustix::fs::fstat(&fd)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|err| unreadable(path, std::io::Error::from(err).to_string()))?;
    if filetype != rustix::fs::FileType::RegularFile {
        return Err(unreadable(path, "not_a_file"));
    }
    let mut file = fs::File::from(fd);
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        check_deadline(deadline)?;
        let remaining = max_bytes
            .saturating_add(1)
            .saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
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
        bytes.extend_from_slice(&buffer[..count]);
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(unreadable(path, "oversize"));
        }
    }
    Ok(bytes)
}

fn check_deadline(
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| crate::internal::internal(&error.to_string()))?;
    }
    Ok(())
}

fn unreadable(path: &Path, problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), problem)
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
