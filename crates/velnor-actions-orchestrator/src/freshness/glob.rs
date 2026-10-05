//! Prefix-pruned, bounded Python-compatible source-scope glob matching.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct Entry {
    path: String,
    pub(super) is_file: bool,
}

const MAX_GLOB_PATTERN_BYTES: usize = 4096;
const MAX_GLOB_COMPONENTS: usize = 256;
const MAX_GLOB_VISITS: usize = 100_000;

#[derive(Debug)]
struct Child {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

struct PreparedPattern<'a> {
    components: Vec<&'a str>,
    prefix_len: usize,
    base: PathBuf,
    visited: usize,
}

pub(super) fn matching_entries(root: &Path, pattern: &str) -> Result<Vec<Entry>, String> {
    matching_entries_with_limit(root, pattern, MAX_GLOB_VISITS)
}

fn matching_entries_with_limit(
    root: &Path,
    pattern: &str,
    visit_limit: usize,
) -> Result<Vec<Entry>, String> {
    let canonical_root = fs::canonicalize(root).map_err(|error| {
        format!(
            "cannot resolve repository root {} ({error})",
            root.display()
        )
    })?;
    let Some(PreparedPattern {
        components,
        prefix_len,
        base,
        mut visited,
    }) = prepare_pattern(root, &canonical_root, pattern, visit_limit)?
    else {
        return Ok(Vec::new());
    };
    if prefix_len == components.len() {
        return matched_entry(root, &base).map(|entry| entry.into_iter().collect());
    }
    walk_pattern(
        root,
        &canonical_root,
        &components,
        prefix_len,
        base,
        &mut visited,
        visit_limit,
    )
}

fn prepare_pattern<'a>(
    root: &Path,
    canonical_root: &Path,
    pattern: &'a str,
    visit_limit: usize,
) -> Result<Option<PreparedPattern<'a>>, String> {
    if pattern.is_empty() || pattern.len() > MAX_GLOB_PATTERN_BYTES || pattern.starts_with('/') {
        return Err("invalid or oversized glob pattern".to_owned());
    }
    let components: Vec<&str> = pattern.split('/').collect();
    if components.len() > MAX_GLOB_COMPONENTS
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return Err("invalid glob path components".to_owned());
    }
    let prefix_len = components
        .iter()
        .take_while(|component| !component_has_magic(component))
        .count();
    let mut visited = 0;
    let mut base = root.to_path_buf();
    for (index, component) in components[..prefix_len].iter().enumerate() {
        bump_visits(&mut visited, visit_limit)?;
        base.push(component);
        if index + 1 < components.len() && !directory_within_root(canonical_root, &base)? {
            return Ok(None);
        }
    }
    Ok(Some(PreparedPattern {
        components,
        prefix_len,
        base,
        visited,
    }))
}

fn walk_pattern(
    root: &Path,
    canonical_root: &Path,
    components: &[&str],
    prefix_len: usize,
    base: PathBuf,
    visited: &mut usize,
    visit_limit: usize,
) -> Result<Vec<Entry>, String> {
    if !directory_within_root(canonical_root, &base)? {
        return Ok(Vec::new());
    }

    let mut pending = vec![(base, prefix_len)];
    let mut entries = BTreeMap::new();
    while let Some((path, component_index)) = pending.pop() {
        bump_visits(visited, visit_limit)?;
        if component_index == components.len() {
            if let Some(entry) = matched_entry(root, &path)? {
                entries.entry(entry.path.clone()).or_insert(entry);
            }
            continue;
        }
        let component = components[component_index];
        if component == "**" {
            pending.push((path.clone(), component_index + 1));
            for child in read_children(canonical_root, &path, visited, visit_limit)? {
                if child.name.starts_with('.') {
                    continue;
                }
                if component_index + 1 == components.len() {
                    pending.push((child.path.clone(), component_index + 1));
                }
                if child.is_dir {
                    pending.push((child.path, component_index));
                }
            }
        } else {
            for child in read_children(canonical_root, &path, visited, visit_limit)? {
                if child.name.starts_with('.') && !component.starts_with('.') {
                    continue;
                }
                if component_matches(component, &child.name) {
                    pending.push((child.path, component_index + 1));
                }
            }
        }
    }
    Ok(entries.into_values().collect())
}

fn read_children(
    canonical_root: &Path,
    directory: &Path,
    visited: &mut usize,
    visit_limit: usize,
) -> Result<Vec<Child>, String> {
    if !directory_within_root(canonical_root, directory)? {
        return Ok(Vec::new());
    }
    let children = match fs::read_dir(directory) {
        Ok(children) => children,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => {
            return Err(format!(
                "cannot enumerate {} ({error})",
                directory.display()
            ));
        }
    };
    let mut output = Vec::new();
    for child in children {
        bump_visits(visited, visit_limit)?;
        let child = child
            .map_err(|error| format!("cannot read entry in {} ({error})", directory.display()))?;
        let file_type = child
            .file_type()
            .map_err(|error| format!("cannot inspect {} ({error})", child.path().display()))?;
        let path = child.path();
        let is_dir = entry_is_directory(&path, file_type)?;
        output.push(Child {
            path,
            name: child.file_name().to_string_lossy().into_owned(),
            is_dir,
        });
    }
    output.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(output)
}

fn entry_is_directory(path: &Path, file_type: std::fs::FileType) -> Result<bool, String> {
    if file_type.is_dir() {
        return Ok(true);
    }
    if !file_type.is_symlink() {
        return Ok(false);
    }
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect symlink {} ({error})",
            path.display()
        )),
    }
}

fn directory_within_root(canonical_root: &Path, directory: &Path) -> Result<bool, String> {
    let metadata = match fs::metadata(directory) {
        Ok(metadata) => metadata,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(format!("cannot inspect {} ({error})", directory.display())),
    };
    if !metadata.is_dir() {
        return Ok(false);
    }
    let canonical = fs::canonicalize(directory)
        .map_err(|error| format!("cannot resolve {} ({error})", directory.display()))?;
    if !canonical.starts_with(canonical_root) {
        return Err(format!(
            "glob traversal escapes repository root at {}",
            directory.display()
        ));
    }
    Ok(true)
}

fn matched_entry(root: &Path, path: &Path) -> Result<Option<Entry>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect {} ({error})", path.display())),
    };
    let is_file = if metadata.file_type().is_symlink() {
        match fs::metadata(path) {
            Ok(target) => target.is_file(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("cannot follow {} ({error})", path.display())),
        }
    } else {
        metadata.is_file()
    };
    let relative = path
        .strip_prefix(root)
        .map_err(|error| format!("matched path escaped repository root ({error})"))?;
    let relative = relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    Ok(Some(Entry {
        path: relative,
        is_file,
    }))
}

fn bump_visits(visited: &mut usize, limit: usize) -> Result<(), String> {
    *visited = visited
        .checked_add(1)
        .ok_or_else(|| "glob traversal counter overflowed".to_owned())?;
    if *visited > limit {
        return Err(format!(
            "glob traversal exceeded {limit} filesystem entries"
        ));
    }
    Ok(())
}

fn component_has_magic(component: &str) -> bool {
    component
        .chars()
        .any(|character| matches!(character, '*' | '?' | '['))
}

fn component_matches(pattern: &str, value: &str) -> bool {
    if value.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    glob_component(pattern, value)
}

fn glob_component(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut matches = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    matches[pattern.len()][value.len()] = true;
    for pattern_index in (0..pattern.len()).rev() {
        if pattern[pattern_index] == '*' {
            for value_index in (0..=value.len()).rev() {
                matches[pattern_index][value_index] = matches[pattern_index + 1][value_index]
                    || (value_index < value.len() && matches[pattern_index][value_index + 1]);
            }
            continue;
        }
        for value_index in 0..value.len() {
            if pattern[pattern_index] == '?' {
                matches[pattern_index][value_index] = matches[pattern_index + 1][value_index + 1];
            } else if pattern[pattern_index] == '[' {
                if let Some((next, class_matches)) =
                    character_class(&pattern, pattern_index, value[value_index])
                {
                    matches[pattern_index][value_index] =
                        class_matches && matches[next][value_index + 1];
                } else {
                    matches[pattern_index][value_index] = pattern[pattern_index]
                        == value[value_index]
                        && matches[pattern_index + 1][value_index + 1];
                }
            } else {
                matches[pattern_index][value_index] = pattern[pattern_index] == value[value_index]
                    && matches[pattern_index + 1][value_index + 1];
            }
        }
    }
    matches[0][0]
}

fn character_class(pattern: &[char], start: usize, value: char) -> Option<(usize, bool)> {
    let mut content_start = start + 1;
    let negated = pattern.get(content_start) == Some(&'!');
    if negated {
        content_start += 1;
    }
    let mut end = content_start;
    if pattern.get(end) == Some(&']') {
        end += 1;
    }
    while pattern.get(end).is_some_and(|character| *character != ']') {
        end += 1;
    }
    if end >= pattern.len() {
        return None;
    }
    let found = class_contains(&pattern[content_start..end], value);
    Some((end + 1, found != negated))
}

fn class_contains(class: &[char], value: char) -> bool {
    let mut index = 0;
    while index < class.len() {
        if index + 2 < class.len() && class[index + 1] == '-' {
            if class[index] <= value && value <= class[index + 2] {
                return true;
            }
            index += 3;
        } else {
            if class[index] == value {
                return true;
            }
            index += 1;
        }
    }
    false
}

#[cfg(test)]
#[path = "glob_tests.rs"]
mod tests;
