//! Retirement deletes only exact former output paths carrying canonical markers.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::OrchestratorError;
use crate::apt_delivery::APT_RETIRED_TREE_PATHS;
use velnor_actions_contract::{MARKER_PREFIX, marker_for_version};

use velnor_actions_workflow_renderer::release_tree::RELEASE_RETIRED_TREE_PATHS;

/// Exact old output destinations; no executable compatibility routes.
pub(super) fn is_retired_path(relative: &Path) -> bool {
    APT_RETIRED_TREE_PATHS
        .iter()
        .chain(RELEASE_RETIRED_TREE_PATHS)
        .any(|path| {
            Path::new(path)
                .strip_prefix(".github")
                .is_ok_and(|path| path == relative)
        })
}

/// An unmarked file stays human-owned; malformed claimed ownership fails closed.
pub(super) fn marked_owned(source: &Path) -> Result<bool, OrchestratorError> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::NONBLOCK
        | rustix::fs::OFlags::CLOEXEC;
    let fd = rustix::fs::open(source, flags, rustix::fs::Mode::empty())
        .map_err(|error| super::io(source, std::io::Error::from(error)))?;
    let file = std::fs::File::from(fd);
    if !file
        .metadata()
        .map_err(|error| super::io(source, error))?
        .is_file()
    {
        return Err(super::profile_ownership_error(source, "not_a_regular_file"));
    }
    let mut first = Vec::new();
    BufReader::new(file)
        .take(512)
        .read_until(b'\n', &mut first)
        .map_err(|error| super::io(source, error))?;
    if !first.starts_with(MARKER_PREFIX.as_bytes()) {
        return Ok(false);
    }
    let line = std::str::from_utf8(&first)
        .ok()
        .and_then(|line| line.strip_suffix('\n'));
    let valid = line.is_some_and(|line| {
        line.strip_prefix(MARKER_PREFIX)
            .and_then(|rest| rest.split_once(';'))
            .is_some_and(|(version, _)| {
                marker_for_version(version).is_ok_and(|marker| marker == line)
            })
    });
    if !valid {
        return Err(super::profile_ownership_error(
            source,
            "invalid_retired_marker",
        ));
    }
    Ok(true)
}
