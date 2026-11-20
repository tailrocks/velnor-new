// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;
use crate::tools_local::HOME_PATHS;
use crate::tree_removal::remove_tree;

const FORMAT: &str = "velnor-tools-anchor-v1";
const ANCHOR_MAX_BYTES: u64 = 1024;
const FILE_MAX_BYTES: u64 = 2 << 30;
const TREE_MAX_BYTES: u64 = 16 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub file_bytes: u64,
    pub tree_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: FILE_MAX_BYTES,
            tree_bytes: TREE_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Verb {
    Reset,
    Check,
    Record,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discard {
    NoAnchor,
    BadAnchor,
    Mismatch,
    Unreadable(String),
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Reset,
    NothingRestored,
    Verified,
    Discarded(Discard),
    Recorded,
    Matches,
    Differs(Discard),
}

fn field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn entry(
    hasher: &mut blake3::Hasher,
    root: &Path,
    path: &Path,
    limits: &Limits,
    spent: &mut u64,
) -> Result<(), String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot stat {}: {error}", relative.display()))?;
    let kind = metadata.file_type();
    let mode = (metadata.mode() & 0o7777).to_le_bytes();
    if kind.is_symlink() {
        let target = fs::read_link(path)
            .map_err(|error| format!("cannot read link {}: {error}", relative.display()))?;
        field(hasher, b"l");
        field(hasher, relative.as_os_str().as_bytes());
        field(hasher, target.as_os_str().as_bytes());
    } else if kind.is_file() {
        let length = metadata.len();
        if length > limits.file_bytes {
            return Err(format!(
                "{} is larger than the file limit",
                relative.display()
            ));
        }
        if metadata.blocks().saturating_mul(512) < length {
            return Err(format!("{} is a sparse file", relative.display()));
        }
        *spent = spent.saturating_add(length);
        if *spent > limits.tree_bytes {
            return Err(format!(
                "the tree is larger than the limit at {}",
                relative.display()
            ));
        }
        let file = fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", relative.display()))?;
        let mut content = blake3::Hasher::new();
        let read = std::io::copy(&mut file.take(length.saturating_add(1)), &mut content)
            .map_err(|error| format!("cannot read {}: {error}", relative.display()))?;
        if read != length {
            return Err(format!("{} changed while it was read", relative.display()));
        }
        field(hasher, b"f");
        field(hasher, relative.as_os_str().as_bytes());
        field(hasher, &mode);
        field(hasher, content.finalize().as_bytes());
    } else if kind.is_dir() {
        field(hasher, b"d");
        field(hasher, relative.as_os_str().as_bytes());
        field(hasher, &mode);
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|error| format!("cannot list {}: {error}", relative.display()))?
            .map(|child| child.map(|child| child.path()))
            .collect::<Result<_, _>>()
            .map_err(|error| format!("cannot list {}: {error}", relative.display()))?;
        children.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
        for child in children {
            entry(hasher, root, &child, limits, spent)?;
        }
    } else {
        return Err(format!(
            "{} is not a file, a directory or a link",
            relative.display()
        ));
    }
    Ok(())
}

pub fn digest(home: &Path) -> Result<String, String> {
    digest_within(home, &Limits::default())
}

pub fn digest_within(home: &Path, limits: &Limits) -> Result<String, String> {
    let mut spent = 0;
    let mut hasher = blake3::Hasher::new();
    for path in HOME_PATHS {
        let root = home.join(path);
        field(&mut hasher, path.as_bytes());
        if fs::symlink_metadata(&root).is_err() {
            field(&mut hasher, b"absent");
        } else {
            entry(&mut hasher, home, &root, limits, &mut spent)?;
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[derive(Debug, Clone, Copy)]
pub struct Anchor<'a> {
    pub file: &'a Path,
    pub restored: bool,
}

fn read_anchor(anchor: &Anchor<'_>) -> Result<String, Discard> {
    if !anchor.restored {
        return Err(Discard::NoAnchor);
    }
    let metadata = fs::symlink_metadata(anchor.file).map_err(|_| Discard::NoAnchor)?;
    if !metadata.is_file() || metadata.len() > ANCHOR_MAX_BYTES {
        return Err(Discard::BadAnchor);
    }
    let text = fs::read_to_string(anchor.file).map_err(|_| Discard::BadAnchor)?;
    text.strip_suffix('\n')
        .and_then(|line| line.strip_prefix(FORMAT))
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
        .map(str::to_owned)
        .ok_or(Discard::BadAnchor)
}

fn compare(home: &Path, anchor: &Anchor<'_>) -> Result<(), Discard> {
    let expected = read_anchor(anchor)?;
    let actual = digest(home).map_err(Discard::Unreadable)?;
    if actual == expected {
        Ok(())
    } else {
        Err(Discard::Mismatch)
    }
}

fn discard(home: &Path) -> Result<(), CiError> {
    for path in HOME_PATHS {
        let root = home.join(path);
        remove_tree(&root).map_err(|error| {
            CiError::new(format!(
                "cannot discard the restored {}: {error}",
                root.display()
            ))
        })?;
    }
    Ok(())
}

pub fn reset(anchor: &Path) -> Result<Verdict, CiError> {
    remove_tree(anchor).map_err(|error| {
        CiError::new(format!(
            "cannot clear the anchor {}: {error}",
            anchor.display()
        ))
    })?;
    Ok(Verdict::Reset)
}

pub fn check(home: &Path, anchor: &Anchor<'_>) -> Result<Verdict, CiError> {
    if HOME_PATHS
        .iter()
        .all(|path| fs::symlink_metadata(home.join(path)).is_err())
    {
        return Ok(Verdict::NothingRestored);
    }
    match compare(home, anchor) {
        Ok(()) => Ok(Verdict::Verified),
        Err(reason) => {
            discard(home)?;
            Ok(Verdict::Discarded(reason))
        }
    }
}

pub fn record(home: &Path, anchor: &Anchor<'_>, trusted: bool) -> Result<Verdict, CiError> {
    if !trusted {
        return Ok(Verdict::Differs(Discard::Untrusted));
    }
    match compare(home, anchor) {
        Ok(()) => Ok(Verdict::Matches),
        Err(Discard::NoAnchor) => {
            let actual = match digest(home) {
                Ok(actual) => actual,
                Err(reason) => return Ok(Verdict::Differs(Discard::Unreadable(reason))),
            };
            write_anchor(anchor.file, &actual)?;
            Ok(Verdict::Recorded)
        }
        Err(reason) => Ok(Verdict::Differs(reason)),
    }
}

fn write_anchor(anchor: &Path, digest: &str) -> Result<(), CiError> {
    let fail = |error: std::io::Error| {
        CiError::new(format!(
            "cannot write the anchor {}: {error}",
            anchor.display()
        ))
    };
    let parent = anchor.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(fail)?;
    let staged = parent.join(format!(".anchor-{}", std::process::id()));
    fs::write(&staged, format!("{FORMAT} {digest}\n")).map_err(fail)?;
    fs::rename(&staged, anchor).map_err(fail)
}
