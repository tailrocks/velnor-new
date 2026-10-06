use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Component, Path};

use super::ActionArchiveSeedError;

const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENTS: usize = 500_000;
const MAX_LINK_EXPANSIONS: usize = 40;

#[derive(Debug, Clone)]
pub(super) enum EntryKind {
    File,
    Directory,
    Symlink(String),
}

#[derive(Debug, Clone)]
enum PathToken {
    Normal(String),
    Parent,
    Current,
}

#[derive(Default)]
struct PathNode<'a> {
    children: BTreeMap<&'a str, PathNode<'a>>,
    is_non_directory: bool,
}

pub(super) fn entry_kind<R: Read>(
    entry_type: tar::EntryType,
    entry: &tar::Entry<'_, R>,
    metadata_bytes: &mut usize,
) -> Result<EntryKind, ActionArchiveSeedError> {
    if entry_type.is_dir() {
        return Ok(EntryKind::Directory);
    }
    if entry_type.is_file() {
        return Ok(EntryKind::File);
    }
    if entry_type.is_symlink() {
        let target = entry
            .link_name()
            .map_err(|_| ActionArchiveSeedError::UnsafeEntry)?
            .ok_or(ActionArchiveSeedError::UnsafeEntry)?;
        let target = target
            .to_str()
            .ok_or(ActionArchiveSeedError::UnsafeEntry)?
            .to_owned();
        *metadata_bytes = metadata_bytes
            .checked_add(target.len())
            .ok_or(ActionArchiveSeedError::SizeLimit)?;
        if target.len() > MAX_PATH_BYTES {
            return Err(ActionArchiveSeedError::SizeLimit);
        }
        return Ok(EntryKind::Symlink(target));
    }
    Err(ActionArchiveSeedError::UnsafeEntry)
}

pub(super) fn validate_members(
    members: Vec<(String, EntryKind)>,
) -> Result<(), ActionArchiveSeedError> {
    let wrapper = common_wrapper(&members);
    let mut entries = BTreeMap::new();
    for (path, kind) in members {
        let name = strip_wrapper(&path, wrapper.as_deref(), &kind)?;
        if name.is_empty() {
            continue;
        }
        if entries.insert(name, kind).is_some() {
            return Err(ActionArchiveSeedError::UnsafeEntry);
        }
    }
    validate_tree(&entries)?;
    validate_link_graph(&entries)
}

pub(super) fn safe_path(path: &Path) -> Result<String, ActionArchiveSeedError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ActionArchiveSeedError::UnsafeEntry);
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let value = part.to_str().ok_or(ActionArchiveSeedError::UnsafeEntry)?;
                if value.contains('\0') {
                    return Err(ActionArchiveSeedError::UnsafeEntry);
                }
                parts.push(value);
            }
            _ => return Err(ActionArchiveSeedError::UnsafeEntry),
        }
    }
    if parts.is_empty() {
        return Err(ActionArchiveSeedError::UnsafeEntry);
    }
    let value = parts.join("/");
    if value.len() > MAX_PATH_BYTES {
        return Err(ActionArchiveSeedError::SizeLimit);
    }
    Ok(value)
}

fn common_wrapper(members: &[(String, EntryKind)]) -> Option<String> {
    let first = members.first()?.0.split('/').next()?.to_owned();
    let has_child = members.iter().any(|(path, _)| path.contains('/'));
    if has_child
        && members
            .iter()
            .all(|(path, _)| path == &first || path.starts_with(&format!("{first}/")))
    {
        Some(first)
    } else {
        None
    }
}

fn strip_wrapper(
    path: &str,
    wrapper: Option<&str>,
    kind: &EntryKind,
) -> Result<String, ActionArchiveSeedError> {
    let Some(wrapper) = wrapper else {
        return Ok(path.to_owned());
    };
    if path == wrapper {
        return if matches!(kind, EntryKind::Directory) {
            Ok(String::new())
        } else {
            Err(ActionArchiveSeedError::UnsafeEntry)
        };
    }
    path.strip_prefix(&format!("{wrapper}/"))
        .map(str::to_owned)
        .ok_or(ActionArchiveSeedError::UnsafeEntry)
}

fn validate_tree(entries: &BTreeMap<String, EntryKind>) -> Result<(), ActionArchiveSeedError> {
    let mut root = PathNode::default();
    let mut components = 0_usize;
    for (path, kind) in entries {
        components = components
            .checked_add(path.split('/').count())
            .ok_or(ActionArchiveSeedError::SizeLimit)?;
        if components > MAX_PATH_COMPONENTS {
            return Err(ActionArchiveSeedError::SizeLimit);
        }
        let mut node = &mut root;
        for component in path.split('/') {
            node = node.children.entry(component).or_default();
        }
        node.is_non_directory = !matches!(kind, EntryKind::Directory);
    }
    let mut pending = vec![&root];
    while let Some(node) = pending.pop() {
        if node.is_non_directory && !node.children.is_empty() {
            return Err(ActionArchiveSeedError::UnsafeEntry);
        }
        pending.extend(node.children.values());
    }
    Ok(())
}

fn validate_link_graph(
    entries: &BTreeMap<String, EntryKind>,
) -> Result<(), ActionArchiveSeedError> {
    let links = entries
        .iter()
        .filter_map(|(path, kind)| match kind {
            EntryKind::Symlink(target) => Some((path.clone(), target.clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    for (path, target) in &links {
        resolve_link(path, target, &links)?;
    }
    Ok(())
}

fn resolve_link(
    path: &str,
    target: &str,
    links: &BTreeMap<String, String>,
) -> Result<(), ActionArchiveSeedError> {
    let mut resolved = path
        .split('/')
        .take(path.split('/').count().saturating_sub(1))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut pending = path_tokens(Path::new(target))?;
    let mut expansions = 0_usize;
    while let Some(token) = pending.pop_front() {
        match token {
            PathToken::Parent if resolved.pop().is_none() => {
                return Err(ActionArchiveSeedError::UnsafeEntry);
            }
            PathToken::Parent | PathToken::Current => {}
            PathToken::Normal(part) => {
                resolved.push(part);
                let candidate = resolved.join("/");
                if let Some(next_target) = links.get(&candidate) {
                    expansions += 1;
                    if expansions > MAX_LINK_EXPANSIONS {
                        return Err(ActionArchiveSeedError::UnsafeEntry);
                    }
                    resolved.pop();
                    let next = path_tokens(Path::new(next_target))?;
                    for item in next.into_iter().rev() {
                        pending.push_front(item);
                    }
                }
            }
        }
    }
    if resolved.is_empty() {
        return Err(ActionArchiveSeedError::UnsafeEntry);
    }
    Ok(())
}

fn path_tokens(path: &Path) -> Result<VecDeque<PathToken>, ActionArchiveSeedError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ActionArchiveSeedError::UnsafeEntry);
    }
    path.components()
        .map(|component| match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or(ActionArchiveSeedError::UnsafeEntry)?;
                if part.contains('\0') {
                    return Err(ActionArchiveSeedError::UnsafeEntry);
                }
                Ok(PathToken::Normal(part.to_owned()))
            }
            Component::ParentDir => Ok(PathToken::Parent),
            Component::CurDir => Ok(PathToken::Current),
            _ => Err(ActionArchiveSeedError::UnsafeEntry),
        })
        .collect()
}
