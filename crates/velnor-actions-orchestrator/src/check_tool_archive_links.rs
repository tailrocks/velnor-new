use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::OrchestratorError;
use crate::internal::internal;
use velnor_actions_mise::CheckDeadline;

use super::{io_error, unsafe_path_text};

pub(super) fn discard_entry<R: Read>(
    reader: &mut R,
    size: u64,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    super::check_deadline(deadline)?;
    let copied = io::copy(&mut reader.take(size), &mut io::sink())
        .map_err(|error| internal(&format!("tool_archive:{error}")))?;
    if copied == size {
        Ok(())
    } else {
        Err(internal("tool_archive_truncated_entry"))
    }
}

/// A link held until every regular file and directory has been staged.
#[derive(Debug)]
pub(super) struct PendingLink {
    pub(super) path: PathBuf,
    pub(super) target: String,
}

pub(super) fn target_from_bytes(raw: &[u8]) -> Result<String, OrchestratorError> {
    let target = std::str::from_utf8(raw).map_err(|_| internal("tool_archive_non_utf8_link"))?;
    validate_target(target)?;
    Ok(target.to_owned())
}

pub(super) fn read_target<R: Read>(
    reader: &mut R,
    size: u64,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    super::check_deadline(deadline)?;
    if size > u64::try_from(super::MAX_PATH_BYTES).unwrap_or(u64::MAX) {
        return Err(internal("tool_archive_link_size_limit"));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    let copied = reader
        .take(size)
        .read_to_end(&mut bytes)
        .map_err(|error| internal(&format!("tool_archive_link:{error}")))?;
    if u64::try_from(copied).unwrap_or(u64::MAX) != size {
        return Err(internal("tool_archive_truncated_link"));
    }
    super::check_deadline(deadline)?;
    target_from_bytes(&bytes)
}

fn validate_target(target: &str) -> Result<(), OrchestratorError> {
    if target.is_empty()
        || target.len() > super::MAX_PATH_BYTES
        || target.contains('\0')
        || target.contains('\\')
        || target.starts_with('/')
        || target.as_bytes().get(1) == Some(&b':')
    {
        return Err(unsafe_path_text(target, "archive_link_target"));
    }
    if target
        .split('/')
        .any(|component| component.is_empty() || component == ".")
    {
        return Err(unsafe_path_text(target, "archive_link_target"));
    }
    Ok(())
}

pub(super) fn create_links(
    destination: &Path,
    links: Vec<PendingLink>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    super::check_deadline(deadline)?;
    if links.is_empty() {
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = (destination, links);
        return Err(OrchestratorError::unsupported(
            "tool_archive_symlink",
            "platform_has_no_posix_symlink_projection",
        ));
    }
    #[cfg(unix)]
    {
        let targets = normalized_targets(&links, deadline)?;
        validate_links(destination, &targets, deadline)?;
        for link in links {
            super::check_deadline(deadline)?;
            let parent = link.path.parent().unwrap_or_else(|| Path::new(""));
            super::paths::ensure_directory(destination, parent)?;
            let output = destination.join(&link.path);
            if fs::symlink_metadata(&output).is_ok() {
                return Err(OrchestratorError::OverwriteRefused {
                    path: output.display().to_string(),
                });
            }
            std::os::unix::fs::symlink(&link.target, &output)
                .map_err(|error| io_error(&output, error))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn normalized_targets(
    links: &[PendingLink],
    deadline: CheckDeadline,
) -> Result<HashMap<PathBuf, PathBuf>, OrchestratorError> {
    let mut targets = HashMap::new();
    for link in links {
        super::check_deadline(deadline)?;
        let target = normalize_target(&link.path, &link.target)?;
        if targets.insert(link.path.clone(), target).is_some() {
            return Err(internal("tool_archive_duplicate_path"));
        }
    }
    Ok(targets)
}

#[cfg(unix)]
fn normalize_target(link: &Path, target: &str) -> Result<PathBuf, OrchestratorError> {
    let mut path = link.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    for component in target.split('/') {
        if component == ".." {
            if !path.pop() {
                return Err(unsafe_path_text(target, "archive_link_escape"));
            }
        } else {
            path.push(component);
        }
    }
    if path.as_os_str().is_empty() {
        return Err(unsafe_path_text(target, "archive_link_target"));
    }
    Ok(path)
}

#[cfg(unix)]
fn validate_links(
    destination: &Path,
    targets: &HashMap<PathBuf, PathBuf>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for (path, target) in targets {
        super::check_deadline(deadline)?;
        reject_link_parent(targets, path, deadline)?;
        reject_link_component(targets, target, deadline)?;
        let mut states = HashMap::new();
        resolve_target(destination, target, targets, &mut states, 0, deadline)?;
    }
    Ok(())
}

#[cfg(unix)]
fn reject_link_parent(
    links: &HashMap<PathBuf, PathBuf>,
    path: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut current = PathBuf::new();
    for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
        super::check_deadline(deadline)?;
        let Component::Normal(part) = component else {
            return Err(internal("tool_archive_link_path"));
        };
        current.push(part);
        if links.contains_key(&current) {
            return Err(internal("tool_archive_link_parent"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn reject_link_component(
    links: &HashMap<PathBuf, PathBuf>,
    path: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut current = PathBuf::new();
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        super::check_deadline(deadline)?;
        let Component::Normal(part) = component else {
            return Err(internal("tool_archive_link_target"));
        };
        current.push(part);
        if index + 1 != components.len() && links.contains_key(&current) {
            return Err(internal("tool_archive_link_target_component"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn resolve_target(
    destination: &Path,
    path: &Path,
    links: &HashMap<PathBuf, PathBuf>,
    states: &mut HashMap<PathBuf, LinkState>,
    depth: usize,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    super::check_deadline(deadline)?;
    if depth > super::MAX_PATH_COMPONENTS {
        return Err(internal("tool_archive_link_depth"));
    }
    match states.get(path) {
        Some(LinkState::Visiting) => return Err(internal("tool_archive_link_cycle")),
        Some(LinkState::Done) => return Ok(()),
        None => {}
    }
    states.insert(path.to_path_buf(), LinkState::Visiting);
    if let Some(next) = links.get(path) {
        resolve_target(destination, next, links, states, depth + 1, deadline)?;
    } else {
        let output = destination.join(path);
        let metadata = fs::symlink_metadata(&output).map_err(|error| io_error(&output, error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(internal("tool_archive_link_dangling_or_nonregular"));
        }
    }
    states.insert(path.to_path_buf(), LinkState::Done);
    Ok(())
}

#[cfg(unix)]
enum LinkState {
    Visiting,
    Done,
}
