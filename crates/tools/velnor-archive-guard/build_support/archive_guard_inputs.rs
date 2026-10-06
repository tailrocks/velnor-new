use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const MANIFEST: &str = "scripts/archive-guard-inputs.txt";
const MANIFEST_LIMIT: u64 = 32 * 1024;
const FILE_LIMIT: u64 = 16 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 32 * 1024 * 1024;
const FILE_COUNT_LIMIT: usize = 512;
const DIRECTORY_COUNT_LIMIT: usize = 512;
const DISCOVERED_ENTRY_LIMIT: usize = 512;

#[derive(Clone, Copy)]
enum Kind {
    Required,
    Optional,
    Tree,
}

struct Entry {
    kind: Kind,
    path: String,
}

pub(crate) fn fingerprint(root: &Path) -> Result<(String, Vec<PathBuf>), Box<dyn Error>> {
    let manifest_path = root.join(MANIFEST);
    let manifest = read_file(root, MANIFEST, MANIFEST_LIMIT)?;
    let entries = parse_manifest(&manifest)?;
    let mut digest = Sha256::new();
    update(&mut digest, MANIFEST, &manifest);
    let mut watched = vec![manifest_path];
    let mut files_seen = HashSet::new();
    let mut total = u64::try_from(manifest.len())?;
    for entry in entries {
        match entry.kind {
            Kind::Required => {
                add_file(
                    root,
                    &entry.path,
                    &mut digest,
                    &mut watched,
                    &mut files_seen,
                    &mut total,
                )?;
            }
            Kind::Optional => {
                if !files_seen.insert(entry.path.clone()) {
                    return Err(invalid("archive input manifest resolves duplicate files").into());
                }
                update_optional(root, &entry.path, &mut digest, &mut watched, &mut total)?;
            }
            Kind::Tree => {
                let directory = root.join(&entry.path);
                watched.push(directory);
                for path in collect_tree_files(root, &entry.path)? {
                    add_file(
                        root,
                        &path,
                        &mut digest,
                        &mut watched,
                        &mut files_seen,
                        &mut total,
                    )?;
                }
            }
        }
    }
    let mut fingerprint = String::with_capacity(64);
    for byte in digest.finalize() {
        fingerprint.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        fingerprint.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    Ok((fingerprint, watched))
}

pub(crate) fn collect_tree_files(
    root: &Path,
    relative: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let directory = safe_relative(relative)?;
    let mut pending = vec![directory];
    let mut files = Vec::new();
    let mut directories = 0;
    let mut discovered = 0;
    while let Some(path) = pending.pop() {
        directories += 1;
        if directories > DIRECTORY_COUNT_LIMIT {
            return Err(invalid("source tree directory count limit exceeded").into());
        }
        let metadata = fs::symlink_metadata(root.join(&path))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("source tree contains an unsafe directory").into());
        }
        for item in fs::read_dir(root.join(&path))? {
            discovered += 1;
            if discovered > DISCOVERED_ENTRY_LIMIT {
                return Err(invalid("source tree entry count limit exceeded").into());
            }
            let child = item?.path();
            let child_metadata = fs::symlink_metadata(&child)?;
            if child_metadata.file_type().is_symlink() {
                return Err(invalid("source tree contains a symlink").into());
            }
            let child_relative = child
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| invalid("source path is not UTF-8"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if child_metadata.is_dir() {
                if pending.len() >= DIRECTORY_COUNT_LIMIT {
                    return Err(invalid("source tree pending directory limit exceeded").into());
                }
                pending.push(PathBuf::from(child_relative));
            } else if child_metadata.is_file() {
                if files.len() >= FILE_COUNT_LIMIT {
                    return Err(invalid("source file count limit exceeded").into());
                }
                files.push(child_relative);
            } else {
                return Err(invalid("source tree contains a non-file entry").into());
            }
        }
    }
    files.sort();
    Ok(files)
}

fn parse_manifest(bytes: &[u8]) -> Result<Vec<Entry>, Box<dyn Error>> {
    let text = std::str::from_utf8(bytes)?;
    if !text.is_ascii() {
        return Err(invalid("archive input manifest is not ASCII").into());
    }
    if !text.ends_with('\n') {
        return Err(invalid("archive input manifest must end with a newline").into());
    }
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    for line in text.lines() {
        let (kind, path) = line
            .split_once(' ')
            .ok_or_else(|| invalid("malformed archive input manifest entry"))?;
        if path.contains(' ') {
            return Err(invalid("malformed archive input manifest path").into());
        }
        if !paths.insert(path.to_owned()) {
            return Err(invalid("duplicate archive input manifest path").into());
        }
        let kind = match kind {
            "file" => Kind::Required,
            "optional" => Kind::Optional,
            "tree" => Kind::Tree,
            _ => return Err(invalid("unknown archive input manifest entry").into()),
        };
        safe_relative(path)?;
        entries.push(Entry {
            kind,
            path: path.to_owned(),
        });
    }
    if entries.is_empty() || entries.len() > FILE_COUNT_LIMIT {
        return Err(invalid("archive input manifest entry count is invalid").into());
    }
    Ok(entries)
}

fn safe_relative(path: &str) -> Result<PathBuf, Box<dyn Error>> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || value.is_absolute()
        || value
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("archive input path is not repository-relative").into());
    }
    Ok(value.to_path_buf())
}

fn add_file(
    root: &Path,
    relative: &str,
    digest: &mut Sha256,
    watched: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    total: &mut u64,
) -> Result<(), Box<dyn Error>> {
    if !seen.insert(relative.to_owned()) {
        return Err(invalid("archive input manifest resolves duplicate files").into());
    }
    if seen.len() > FILE_COUNT_LIMIT {
        return Err(invalid("archive input manifest file count exceeded").into());
    }
    let bytes = read_file(root, relative, FILE_LIMIT)?;
    add_total(total, bytes.len())?;
    update(digest, relative, &bytes);
    watched.push(root.join(relative));
    Ok(())
}

fn update_optional(
    root: &Path,
    relative: &str,
    digest: &mut Sha256,
    watched: &mut Vec<PathBuf>,
    total: &mut u64,
) -> Result<(), Box<dyn Error>> {
    let path = safe_relative(relative)?;
    let full = root.join(path);
    watched.push(full.clone());
    match read_file(root, relative, FILE_LIMIT) {
        Ok(bytes) => {
            add_total(total, bytes.len())?;
            digest.update(relative.as_bytes());
            digest.update([0, 1]);
            digest.update(bytes);
        }
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            digest.update(relative.as_bytes());
            digest.update([0, 0]);
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn read_file(root: &Path, relative: &str, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = safe_relative(relative)?;
    let mut current = root.to_path_buf();
    let components = path.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("archive input parent is not a trusted directory").into());
        }
    }
    current.push(
        path.file_name()
            .ok_or_else(|| invalid("archive input path has no filename"))?,
    );
    let metadata = fs::symlink_metadata(&current)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("archive input file is unsafe or too large").into());
    }
    let file = fs::File::open(current)?;
    let mut bounded = file.take(limit.saturating_add(1));
    let mut bytes = Vec::new();
    bounded.read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > limit {
        return Err(invalid("archive input file is unsafe or too large").into());
    }
    Ok(bytes)
}

fn add_total(total: &mut u64, length: usize) -> Result<(), Box<dyn Error>> {
    *total = total
        .checked_add(u64::try_from(length)?)
        .ok_or_else(|| invalid("archive source size overflow"))?;
    if *total > TOTAL_LIMIT {
        return Err(invalid("archive source size limit exceeded").into());
    }
    Ok(())
}

fn update(digest: &mut Sha256, path: &str, bytes: &[u8]) {
    digest.update(path.as_bytes());
    digest.update([0]);
    digest.update(bytes);
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
