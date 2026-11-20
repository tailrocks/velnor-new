// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Checks the restored tool directories against the anchor a trusted run recorded right after
//! it installed them, before any project code ran. The anchor is one BLAKE3 digest over every
//! entry of `~/.local/share/mise` and `~/.rustup`: path, kind, mode, and the bytes of each file
//! or the target of each link. A restore that differs, or that has no anchor to compare with,
//! is deleted whole so the normal install runs.

use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;
use crate::tools_local::HOME_PATHS;

const FORMAT: &str = "velnor-tools-anchor-v1";
const ANCHOR_MAX_BYTES: u64 = 1024;
pub const RESTORED_ENV: &str = "VELNOR_TOOLS_RESTORED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Verb {
    Check,
    Record,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discard {
    NoAnchor,
    BadAnchor,
    Mismatch,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
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

fn entry(hasher: &mut blake3::Hasher, root: &Path, path: &Path) -> Result<(), String> {
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
        let mut content = blake3::Hasher::new();
        content
            .update_reader(
                fs::File::open(path)
                    .map_err(|error| format!("cannot open {}: {error}", relative.display()))?,
            )
            .map_err(|error| format!("cannot read {}: {error}", relative.display()))?;
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
            entry(hasher, root, &child)?;
        }
    } else {
        return Err(format!(
            "{} is not a file, a directory or a link",
            relative.display()
        ));
    }
    Ok(())
}

/// The digest of the two tool directories under `home`; an absent directory counts as absent.
pub fn digest(home: &Path) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    for path in HOME_PATHS {
        let root = home.join(path);
        field(&mut hasher, path.as_bytes());
        if fs::symlink_metadata(&root).is_err() {
            field(&mut hasher, b"absent");
        } else {
            entry(&mut hasher, home, &root)?;
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn read_anchor(anchor: &Path) -> Result<String, Discard> {
    let metadata = fs::symlink_metadata(anchor).map_err(|_| Discard::NoAnchor)?;
    if !metadata.is_file() || metadata.len() > ANCHOR_MAX_BYTES {
        return Err(Discard::BadAnchor);
    }
    let text = fs::read_to_string(anchor).map_err(|_| Discard::BadAnchor)?;
    text.strip_suffix('\n')
        .and_then(|line| line.strip_prefix(FORMAT))
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
        .map(str::to_owned)
        .ok_or(Discard::BadAnchor)
}

fn compare(home: &Path, anchor: &Path) -> Result<(), Discard> {
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
        let Ok(metadata) = fs::symlink_metadata(&root) else {
            continue;
        };
        let removed = if metadata.is_dir() {
            fs::remove_dir_all(&root)
        } else {
            fs::remove_file(&root)
        };
        removed.map_err(|error| {
            CiError::new(format!(
                "cannot discard the restored {}: {error}",
                root.display()
            ))
        })?;
    }
    Ok(())
}

/// After a restore: keeps it when it equals the anchor, otherwise deletes both directories.
pub fn check(home: &Path, anchor: &Path, restored: bool) -> Result<Verdict, CiError> {
    if !restored {
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

/// After the install, on a trusted run: writes the anchor when there is none, and says whether
/// the tools may be saved, which they may only when they equal the anchor.
pub fn record(home: &Path, anchor: &Path) -> Result<Verdict, CiError> {
    match compare(home, anchor) {
        Ok(()) => Ok(Verdict::Matches),
        Err(Discard::NoAnchor) => {
            let actual = match digest(home) {
                Ok(actual) => actual,
                Err(reason) => return Ok(Verdict::Differs(Discard::Unreadable(reason))),
            };
            write_anchor(anchor, &actual)?;
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

fn reason(discard: &Discard) -> String {
    match discard {
        Discard::NoAnchor => "no anchor was recorded for this key".to_owned(),
        Discard::BadAnchor => "the anchor is malformed".to_owned(),
        Discard::Mismatch => "they differ from the anchor".to_owned(),
        Discard::Unreadable(why) => why.clone(),
    }
}

#[must_use]
pub fn message(verdict: &Verdict) -> String {
    match verdict {
        Verdict::NothingRestored => {
            "velnor: no tools were restored, so none are verified".to_owned()
        }
        Verdict::Verified => "velnor: the restored tools equal the anchor".to_owned(),
        Verdict::Discarded(why) => format!(
            "::notice::the restored tools were discarded ({}); they install from scratch",
            reason(why)
        ),
        Verdict::Recorded => "velnor: recorded the tools anchor".to_owned(),
        Verdict::Matches => "velnor: the tools equal the anchor, so they may be saved".to_owned(),
        Verdict::Differs(why) => format!("::notice::the tools are not saved ({})", reason(why)),
    }
}

fn outputs(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Verified => "verified=true\n".to_owned(),
        Verdict::NothingRestored | Verdict::Discarded(_) => "verified=false\n".to_owned(),
        Verdict::Recorded => "matches=true\nrecorded=true\n".to_owned(),
        Verdict::Matches => "matches=true\nrecorded=false\n".to_owned(),
        Verdict::Differs(_) => "matches=false\nrecorded=false\n".to_owned(),
    }
}

pub fn setup(verb: Verb, anchor: &Path) -> Result<String, CiError> {
    let var = |name: &str| std::env::var(name).ok();
    let home = PathBuf::from(var("HOME").ok_or_else(|| CiError::new("set HOME"))?);
    let verdict = match verb {
        Verb::Check => check(&home, anchor, var(RESTORED_ENV).as_deref() == Some("true"))?,
        Verb::Record => record(&home, anchor)?,
    };
    let file = var("GITHUB_OUTPUT").ok_or_else(|| CiError::new("set GITHUB_OUTPUT"))?;
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)
        .and_then(|mut handle| handle.write_all(outputs(&verdict).as_bytes()))
        .map_err(|error| CiError::new(format!("cannot write {file}: {error}")))?;
    Ok(format!("{}\n", message(&verdict)))
}
