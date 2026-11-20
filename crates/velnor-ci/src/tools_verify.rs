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
    Read,
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
    Anchored(String),
    Unanchored(Discard),
    NothingRestored,
    Verified,
    Discarded(Discard),
    Recorded(String),
    Matches(String),
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
pub struct Anchor<'a>(pub &'a str);

#[derive(Debug, Clone, Copy)]
pub struct Recording<'a> {
    pub anchor: Anchor<'a>,
    pub file: &'a Path,
    pub trusted: bool,
    pub verified: bool,
}

fn is_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn expected(anchor: &Anchor<'_>) -> Result<String, Discard> {
    match anchor.0 {
        "" => Err(Discard::NoAnchor),
        digest if is_digest(digest) => Ok(digest.to_owned()),
        _ => Err(Discard::BadAnchor),
    }
}

pub fn read(file: &Path, matched: bool) -> Verdict {
    if !matched {
        return Verdict::Unanchored(Discard::NoAnchor);
    }
    let Ok(metadata) = fs::symlink_metadata(file) else {
        return Verdict::Unanchored(Discard::NoAnchor);
    };
    if !metadata.is_file() || metadata.len() > ANCHOR_MAX_BYTES {
        return Verdict::Unanchored(Discard::BadAnchor);
    }
    fs::read_to_string(file)
        .ok()
        .and_then(|text| {
            text.strip_suffix('\n')
                .and_then(|line| line.strip_prefix(FORMAT))
                .and_then(|rest| rest.strip_prefix(' '))
                .filter(|hex| is_digest(hex))
                .map(str::to_owned)
        })
        .map_or(Verdict::Unanchored(Discard::BadAnchor), Verdict::Anchored)
}

fn compare(home: &Path, anchor: &Anchor<'_>) -> Result<(), Discard> {
    let expected = expected(anchor)?;
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

pub fn record(home: &Path, recording: &Recording<'_>) -> Result<Verdict, CiError> {
    if !recording.trusted {
        return Ok(Verdict::Differs(Discard::Untrusted));
    }
    let actual = match digest(home) {
        Ok(actual) => actual,
        Err(reason) => return Ok(Verdict::Differs(Discard::Unreadable(reason))),
    };
    if expected(&recording.anchor).is_ok_and(|anchor| anchor == actual) {
        return Ok(Verdict::Matches(actual));
    }
    if recording.verified {
        return Ok(Verdict::Differs(Discard::Mismatch));
    }
    write_anchor(recording.file, &actual)?;
    Ok(Verdict::Recorded(actual))
}

fn write_anchor(anchor: &Path, digest: &str) -> Result<(), CiError> {
    let fail = |error: std::io::Error| {
        CiError::new(format!(
            "cannot write the anchor {}: {error}",
            anchor.display()
        ))
    };
    let parent = anchor.parent().unwrap_or(Path::new("."));
    remove_tree(parent).map_err(fail)?;
    fs::create_dir_all(parent).map_err(fail)?;
    let staged = parent.join(format!(".anchor-{}", std::process::id()));
    fs::write(&staged, format!("{FORMAT} {digest}\n")).map_err(fail)?;
    fs::rename(&staged, anchor).map_err(fail)
}
