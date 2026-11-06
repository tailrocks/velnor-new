// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

mod identity;

pub(super) use identity::{hex, link_identity, tree_hash};

use super::CollectorError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Entry {
    Directory,
    File { bytes: Vec<u8>, executable: bool },
    Link(PathBuf),
}

pub(super) type Tree = BTreeMap<String, Entry>;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TREE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_PATH_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Default)]
struct ReadBudget {
    bytes: u64,
    entries: usize,
    path_bytes: usize,
}

impl ReadBudget {
    fn account(&mut self, path: &Path, depth: usize) -> Result<(), CollectorError> {
        self.entries = self.entries.saturating_add(1);
        self.path_bytes = self
            .path_bytes
            .saturating_add(path.as_os_str().as_bytes().len());
        if self.entries > MAX_ENTRIES || self.path_bytes > MAX_PATH_BYTES || depth > MAX_DEPTH {
            return Err(CollectorError::operational(
                "tree exceeds its structural limit",
            ));
        }
        Ok(())
    }
}

pub(super) fn copy_contents(source: &Path, destination: &Path) -> Result<(), CollectorError> {
    let metadata = fs::symlink_metadata(source).map_err(CollectorError::operational)?;
    if !metadata.file_type().is_dir()
        || !fs::symlink_metadata(destination)
            .map_err(CollectorError::operational)?
            .file_type()
            .is_dir()
    {
        return Err(CollectorError::operational(
            "tree copy requires real source and destination directories",
        ));
    }
    let mut pending = vec![(
        source.to_path_buf(),
        destination.to_path_buf(),
        0_usize,
        false,
    )];
    let mut budget = ReadBudget::default();
    while let Some((directory, output, depth, finish)) = pending.pop() {
        if finish {
            let metadata = fs::symlink_metadata(&directory).map_err(CollectorError::operational)?;
            fs::set_permissions(output, metadata.permissions())
                .map_err(CollectorError::operational)?;
            continue;
        }
        for entry in fs::read_dir(&directory).map_err(CollectorError::operational)? {
            let entry = entry.map_err(CollectorError::operational)?;
            let path = entry.path();
            let relative = path
                .strip_prefix(source)
                .map_err(CollectorError::operational)?;
            budget.account(relative, depth + 1)?;
            let target = output.join(entry.file_name());
            let metadata = fs::symlink_metadata(&path).map_err(CollectorError::operational)?;
            if metadata.file_type().is_symlink() {
                create_symlink(
                    &fs::read_link(&path).map_err(CollectorError::operational)?,
                    &target,
                )?;
            } else if metadata.file_type().is_dir() {
                fs::create_dir(&target).map_err(CollectorError::operational)?;
                pending.push((path.clone(), target.clone(), depth + 1, true));
                pending.push((path, target, depth + 1, false));
            } else if metadata.file_type().is_file() {
                let remaining = MAX_TREE_BYTES.saturating_sub(budget.bytes);
                let bytes = read_regular(&path, &metadata, remaining)?;
                budget.bytes = budget
                    .bytes
                    .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                fs::write(&target, bytes).map_err(CollectorError::operational)?;
                fs::set_permissions(&target, metadata.permissions())
                    .map_err(CollectorError::operational)?;
            } else {
                return Err(CollectorError::operational(format!(
                    "unsupported special file in fixture tree: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path) -> Result<(), CollectorError> {
    std::os::unix::fs::symlink(target, destination).map_err(CollectorError::operational)
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _destination: &Path) -> Result<(), CollectorError> {
    Err(CollectorError::operational(
        "golden collector requires Unix symlink support",
    ))
}

pub(super) fn read_tree(root: &Path) -> Result<Tree, CollectorError> {
    let metadata = fs::symlink_metadata(root).map_err(CollectorError::operational)?;
    if !metadata.file_type().is_dir() {
        return Err(CollectorError::operational(format!(
            "tree root is not a real directory: {}",
            root.display()
        )));
    }
    let mut tree = Tree::new();
    collect_entries(root, &mut tree)?;
    Ok(tree)
}

fn collect_entries(root: &Path, tree: &mut Tree) -> Result<(), CollectorError> {
    let mut budget = ReadBudget::default();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(CollectorError::operational)? {
            let entry = entry.map_err(CollectorError::operational)?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(CollectorError::operational)?;
            budget.account(relative, depth + 1)?;
            let name = relative_text(relative)?;
            let metadata = fs::symlink_metadata(&path).map_err(CollectorError::operational)?;
            let value = if metadata.file_type().is_symlink() {
                Entry::Link(fs::read_link(&path).map_err(CollectorError::operational)?)
            } else if metadata.file_type().is_dir() {
                pending.push((path, depth + 1));
                Entry::Directory
            } else if metadata.file_type().is_file() {
                let remaining = MAX_TREE_BYTES.saturating_sub(budget.bytes);
                let bytes = read_regular(&path, &metadata, remaining)?;
                budget.bytes = budget
                    .bytes
                    .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                Entry::File {
                    bytes,
                    executable: metadata.permissions().mode() & 0o111 != 0,
                }
            } else {
                return Err(CollectorError::operational(format!(
                    "unsupported special file in tree: {}",
                    path.display()
                )));
            };
            tree.insert(name, value);
        }
    }
    Ok(())
}

pub(super) fn compare(left: &Tree, right: &Tree) -> Vec<String> {
    let mut differences = Vec::new();
    let names = left
        .keys()
        .chain(right.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        if left.get(&name) != right.get(&name) {
            differences.push(name);
        }
    }
    differences
}

pub(super) fn replace_regular_files(
    root: &Path,
    replacements: &[(Vec<u8>, Vec<u8>)],
) -> Result<(), CollectorError> {
    let tree = read_tree(root)?;
    for name in tree
        .iter()
        .filter_map(|(name, entry)| matches!(entry, Entry::File { .. }).then_some(name))
    {
        let path = root.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(CollectorError::operational)?;
        let mut file = open_no_follow(&path)?;
        let opened = file.metadata().map_err(CollectorError::operational)?;
        if !same_object(&metadata, &opened) || !opened.file_type().is_file() {
            return Err(CollectorError::operational(
                "generated regular file changed before normalization",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(CollectorError::operational)?;
        for (needle, replacement) in replacements {
            if !needle.is_empty() {
                bytes = replace_bytes(&bytes, needle, replacement);
            }
        }
        file.seek(SeekFrom::Start(0))
            .map_err(CollectorError::operational)?;
        file.set_len(0).map_err(CollectorError::operational)?;
        file.write_all(&bytes)
            .map_err(CollectorError::operational)?;
        if !same_object(
            &opened,
            &file.metadata().map_err(CollectorError::operational)?,
        ) || !same_object(
            &opened,
            &fs::symlink_metadata(&path).map_err(CollectorError::operational)?,
        ) {
            return Err(CollectorError::operational(
                "generated regular file changed during normalization",
            ));
        }
    }
    Ok(())
}

pub(super) fn replace_bytes(content: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return content.to_vec();
    }
    let mut output = Vec::with_capacity(content.len());
    let mut cursor = 0;
    while let Some(offset) = find_bytes(&content[cursor..], needle) {
        let match_start = cursor + offset;
        output.extend_from_slice(&content[cursor..match_start]);
        output.extend_from_slice(replacement);
        cursor = match_start + needle.len();
    }
    output.extend_from_slice(&content[cursor..]);
    output
}

pub(super) fn normalize_plan(raw: &[u8], head: &[u8], root: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    for line in raw.split_inclusive(|byte| *byte == b'\n') {
        let newline = line.ends_with(b"\n");
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let mut content = if content.starts_with(b"Repository: ") {
            b"Repository: <repo>".to_vec()
        } else {
            content.to_vec()
        };
        content = replace_bytes(&content, head, b"<head>");
        if !root.is_empty() {
            content = replace_bytes(&content, root, b"<root>");
        }
        output.extend_from_slice(&content);
        if newline {
            output.push(b'\n');
        }
    }
    output
}

pub(super) fn normalize_generate_stderr(raw: &[u8]) -> Vec<u8> {
    raw.split_inclusive(|byte| *byte == b'\n')
        .flat_map(|line| {
            let newline = line.ends_with(b"\n");
            let content = line.strip_suffix(b"\n").unwrap_or(line);
            let normalized = if content.starts_with(b"Preview: ") {
                b"Preview: <preview>".as_slice()
            } else if content.starts_with(b"Repository: ") {
                b"Repository: <repo>".as_slice()
            } else {
                content
            };
            let mut bytes = normalized.to_vec();
            if newline {
                bytes.push(b'\n');
            }
            bytes
        })
        .collect()
}

pub(super) fn normalize_plan_stderr(raw: &[u8], mappings: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut sorted = mappings.to_vec();
    sorted.sort_by_key(|(needle, _)| std::cmp::Reverse(needle.len()));
    sorted
        .iter()
        .fold(raw.to_vec(), |bytes, (needle, replacement)| {
            replace_bytes(&bytes, needle, replacement)
        })
}

fn relative_text(path: &Path) -> Result<String, CollectorError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| CollectorError::operational("tree path is not UTF-8"))
}

fn read_regular(
    path: &Path,
    initial: &std::fs::Metadata,
    remaining_bytes: u64,
) -> Result<Vec<u8>, CollectorError> {
    let allowed_bytes = MAX_FILE_BYTES.min(remaining_bytes);
    if initial.len() > allowed_bytes {
        return Err(CollectorError::operational("tree exceeds its byte limit"));
    }
    let mut file = open_no_follow(path)?;
    let opened = file.metadata().map_err(CollectorError::operational)?;
    if !same_object(initial, &opened)
        || initial.len() != opened.len()
        || initial.permissions().mode() & 0o111 != opened.permissions().mode() & 0o111
        || !opened.file_type().is_file()
    {
        return Err(CollectorError::operational(
            "regular file changed during tree traversal",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(allowed_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(CollectorError::operational)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > allowed_bytes {
        return Err(CollectorError::operational("tree exceeds its byte limit"));
    }
    let final_opened = file.metadata().map_err(CollectorError::operational)?;
    let final_path = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    if !same_object(&opened, &final_opened)
        || !same_object(&opened, &final_path)
        || opened.len() != final_opened.len()
        || opened.len() != final_path.len()
        || opened.permissions().mode() & 0o111 != final_opened.permissions().mode() & 0o111
        || opened.permissions().mode() & 0o111 != final_path.permissions().mode() & 0o111
    {
        return Err(CollectorError::operational(
            "regular file changed during tree traversal",
        ));
    }
    Ok(bytes)
}

fn open_no_follow(path: &Path) -> Result<File, CollectorError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .or_else(|_| {
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
    })
    .map_err(CollectorError::operational)?;
    Ok(File::from(fd))
}

fn same_object(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino() && left.nlink() == right.nlink()
}

fn find_bytes(content: &[u8], needle: &[u8]) -> Option<usize> {
    content
        .windows(needle.len())
        .position(|window| window == needle)
}
