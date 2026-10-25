#[path = "archive_guard_inputs_filesystem.rs"]
mod filesystem;

use filesystem::{collect_tree_files_from, open_root, read_file, safe_relative};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "scripts/archive-guard-inputs.txt";
const MANIFEST_LIMIT: u64 = 32 * 1024;
const FILE_LIMIT: u64 = 16 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 32 * 1024 * 1024;
const FILE_COUNT_LIMIT: usize = 512;
const DIRECTORY_COUNT_LIMIT: usize = 512;
const DISCOVERED_ENTRY_LIMIT: usize = 1024;

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
    let root_path = root.to_path_buf();
    let root = open_root(root)?;
    let manifest = read_file(&root, MANIFEST, MANIFEST_LIMIT)?;
    let entries = parse_manifest(&manifest)?;
    let mut digest = Sha256::new();
    update(&mut digest, MANIFEST, &manifest);
    let mut watched = vec![root_path.join(MANIFEST)];
    let mut seen = HashSet::new();
    let mut total = u64::try_from(manifest.len())?;
    for entry in entries {
        match entry.kind {
            Kind::Required => add_file(
                &root,
                &root_path,
                &entry.path,
                &mut digest,
                &mut watched,
                &mut seen,
                &mut total,
            )?,
            Kind::Optional => {
                if !seen.insert(entry.path.clone()) || seen.len() > FILE_COUNT_LIMIT {
                    return Err(invalid(
                        "archive input manifest resolves duplicate or excessive files",
                    )
                    .into());
                }
                update_optional(
                    &root,
                    &root_path,
                    &entry.path,
                    &mut digest,
                    &mut watched,
                    &mut total,
                )?;
            }
            Kind::Tree => {
                watched.push(root_path.join(&entry.path));
                for path in collect_tree_files_from(&root, &entry.path)? {
                    add_file(
                        &root,
                        &root_path,
                        &path,
                        &mut digest,
                        &mut watched,
                        &mut seen,
                        &mut total,
                    )?;
                }
            }
        }
    }
    filesystem::verify_root_path(&root_path, &root)?;
    let mut fingerprint = String::with_capacity(64);
    for byte in digest.finalize() {
        use std::fmt::Write as _;
        let _format_result = write!(fingerprint, "{byte:02x}");
    }
    Ok((fingerprint, watched))
}

fn parse_manifest(bytes: &[u8]) -> Result<Vec<Entry>, Box<dyn Error>> {
    let text = std::str::from_utf8(bytes)?;
    if !text.is_ascii() || !text.ends_with('\n') {
        return Err(invalid("archive input manifest must be ASCII and end with a newline").into());
    }
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    for line in text.lines() {
        let (kind, path) = line
            .split_once(' ')
            .ok_or_else(|| invalid("malformed archive input manifest entry"))?;
        if path.contains(' ') || !paths.insert(path.to_owned()) {
            return Err(invalid("malformed or duplicate archive input manifest path").into());
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

fn add_file(
    root: &std::fs::File,
    root_path: &Path,
    relative: &str,
    digest: &mut Sha256,
    watched: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    total: &mut u64,
) -> Result<(), Box<dyn Error>> {
    if !seen.insert(relative.to_owned()) || seen.len() > FILE_COUNT_LIMIT {
        return Err(invalid("archive input manifest resolves duplicate or excessive files").into());
    }
    let bytes = read_file(root, relative, FILE_LIMIT)?;
    add_total(total, bytes.len())?;
    update(digest, relative, &bytes);
    watched.push(root_path.join(relative));
    Ok(())
}

fn update_optional(
    root: &std::fs::File,
    root_path: &Path,
    relative: &str,
    digest: &mut Sha256,
    watched: &mut Vec<PathBuf>,
    total: &mut u64,
) -> Result<(), Box<dyn Error>> {
    safe_relative(relative)?;
    watched.push(root_path.join(relative));
    match read_file(root, relative, FILE_LIMIT) {
        Ok(bytes) => {
            add_total(total, bytes.len())?;
            digest.update(relative.as_bytes());
            digest.update([0, 1]);
            digest.update(bytes);
        }
        Err(error) if filesystem::is_not_found(error.as_ref()) => {
            digest.update(relative.as_bytes());
            digest.update([0, 0]);
        }
        Err(error) => return Err(error),
    }
    Ok(())
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
