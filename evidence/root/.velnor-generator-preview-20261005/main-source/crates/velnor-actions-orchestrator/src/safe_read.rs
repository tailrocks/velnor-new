//! Symlink-rejecting, root-constrained, size-capped file reads (X6).
//!
//! Config, manifest, and event-payload reads all funnel through here.
//! Symlinks reject without reading (even at live targets), repo files
//! canonicalize-and-constrain under the repository root, and every read
//! stops one byte past the bound so oversize files error instead of
//! exhausting memory. Absent files report [`RepoRead::Absent`]; every
//! other failure is an error, never silently masked as absent.
//!
//! Reads are handle-pinned: the final open uses `O_NOFOLLOW`, the open
//! handle must `fstat` as a regular file, and bytes flow only through
//! that handle, so a final-component swap after any pre-check fails
//! the open instead of diverting the read (W6a-G2). Accepted residual:
//! a parent component swapped between containment resolution and the
//! open can divert the open itself; plan-time, same-user only, and the
//! read still matches exactly what was opened.

use std::fs;
use std::io::Read;
use std::path::Path;

use velnor_actions_tofu::{FileCache, PinnedOutcome};

use crate::OrchestratorError;

/// Maximum bytes read from one repo or event file (8 MiB).
///
/// Matches the contract's untrusted-document bound: legitimate config,
/// manifest, and event-payload documents are kilobytes.
pub(crate) const MAX_REPO_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Outcome of a root-constrained repo file read.
#[derive(Debug)]
pub(crate) enum RepoRead {
    /// The file is absent.
    Absent,
    /// The file read within the bound.
    Text(String),
}

/// Read one repo-relative file: absent, or text within `max_bytes`.
///
/// Rejects symlinks, non-files, root escapes, oversize content, and
/// invalid UTF-8 with typed errors; only absence returns [`RepoRead::Absent`].
pub(crate) fn read_repo_file(
    root: &Path,
    rel: &str,
    max_bytes: u64,
) -> Result<RepoRead, OrchestratorError> {
    let path = root.join(rel);
    match fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(RepoRead::Absent),
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
    Ok(RepoRead::Text(read_capped(&canonical, max_bytes)?))
}

/// Read one repo-relative file through the shared pinned compartment.
///
/// Hits return the first read's outcome; misses run [`read_repo_file`]
/// and store its outcome (text, absence, or the error's display
/// string). Callers map the plain outcome onto their own handling;
/// the secure checks always run on the miss path.
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

/// Read one event-payload file outside the checkout, within `max_bytes`.
///
/// The payload lives outside the repository by design, so no root
/// constraint applies; symlinks, non-files, oversize content, and
/// invalid UTF-8 still fail closed as unreadable payloads.
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

/// Read one file through a pinned handle, stopping past `max_bytes`.
///
/// Opens with `O_NOFOLLOW`, requires the open handle to `fstat` as a
/// regular file, and reads only through that handle. Open/fstat
/// failures keep the historical [`std::io::Error`] strings; a symlink
/// swapped in after the pre-checks refuses exactly like a pre-check
/// symlink.
fn read_capped(path: &Path, max_bytes: u64) -> Result<String, OrchestratorError> {
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
    let file = fs::File::from(fd);
    let mut text = String::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(|err| unreadable(path, err.to_string()))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(unreadable(path, "oversize"));
    }
    Ok(text)
}

/// Build an IO error for one path.
fn unreadable(path: &Path, problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), problem)
}

/// Build an unsafe-path error for one path.
fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{MAX_REPO_FILE_BYTES, RepoRead, read_event_file, read_repo_file};

    #[test]
    fn repo_read_round_trip_and_absent() {
        let root = tempfile::TempDir::new().expect("temp root");
        let dir = root.path().join(".velnor");
        fs::create_dir_all(&dir).expect("velnor dir");
        fs::write(dir.join("config.toml"), "schema = 1\n").expect("config");
        assert!(matches!(
            read_repo_file(root.path(), ".velnor/config.toml", MAX_REPO_FILE_BYTES)
                .expect("readable"),
            RepoRead::Text(text) if text == "schema = 1\n"
        ));
        assert!(matches!(
            read_repo_file(root.path(), ".velnor/missing.toml", MAX_REPO_FILE_BYTES)
                .expect("absent"),
            RepoRead::Absent
        ));
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_parent_escape_fails_closed() {
        let root = tempfile::TempDir::new().expect("temp root");
        let outside = tempfile::TempDir::new().expect("outside root");
        fs::write(outside.path().join("config.toml"), "schema = 1\n").expect("outside");
        std::os::unix::fs::symlink(outside.path(), root.path().join(".velnor")).expect("symlink");
        let err = read_repo_file(root.path(), ".velnor/config.toml", MAX_REPO_FILE_BYTES)
            .expect_err("escape refused");
        assert!(err.to_string().contains("root_escape"), "{err}");
        let err = read_event_file(&outside.path().join("config.toml"), MAX_REPO_FILE_BYTES)
            .expect("outside payload reads");
        assert_eq!(err, "schema = 1\n");
    }

    #[test]
    #[cfg(unix)]
    fn repo_symlink_refused_names_reason() {
        let dir = tempfile::TempDir::new().expect("temp root");
        let target = dir.path().join("real.toml");
        fs::write(&target, "schema = 1\n").expect("target");
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = read_repo_file(dir.path(), "link.toml", MAX_REPO_FILE_BYTES)
            .expect_err("symlink refused");
        assert!(err.to_string().contains("symlink_refused"), "{err}");
    }

    #[test]
    fn repo_directory_reports_not_a_file() {
        let dir = tempfile::TempDir::new().expect("temp root");
        fs::create_dir_all(dir.path().join("sub")).expect("subdir");
        let err = read_repo_file(dir.path(), "sub", MAX_REPO_FILE_BYTES).expect_err("dir refused");
        assert!(err.to_string().contains("not_a_file"), "{err}");
    }

    #[test]
    fn repo_oversize_and_bad_utf8_stay_errors() {
        let dir = tempfile::TempDir::new().expect("temp root");
        fs::write(dir.path().join("big.txt"), "0123456789").expect("big");
        let err = read_repo_file(dir.path(), "big.txt", 4).expect_err("oversize refused");
        assert!(err.to_string().contains("oversize"), "{err}");
        fs::write(dir.path().join("bad.txt"), [0xff, 0xfe]).expect("bad");
        let err = read_repo_file(dir.path(), "bad.txt", MAX_REPO_FILE_BYTES)
            .expect_err("bad utf-8 refused");
        assert!(
            !matches!(err, crate::OrchestratorError::UnsafePath { .. }),
            "{err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn event_symlink_fails_closed() {
        let dir = tempfile::TempDir::new().expect("temp root");
        let target = dir.path().join("payload.json");
        fs::write(&target, "{}").expect("payload");
        let link = dir.path().join("event.json");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = read_event_file(&link, MAX_REPO_FILE_BYTES).expect_err("symlink refused");
        assert!(
            err.to_string().contains("unreadable_event_payload"),
            "{err}"
        );
    }
}
