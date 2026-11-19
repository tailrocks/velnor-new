// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use velnor_model::RepoPath;

use crate::error::CiError;

const BASE: &str = "HEAD^1";
const BASE_BYTES_CAP: usize = 4 << 20;
const HEAD: &str = "HEAD";
const MERGED: &str = "HEAD^2";
const SHA_VARIABLE: &str = "GITHUB_SHA";

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| CiError::new(format!("cannot run git: {error}")))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CiError::new(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn checked_out(root: &Path, expected: Option<&str>) -> Result<(), CiError> {
    let head = String::from_utf8_lossy(&git(root, &["rev-parse", "--verify", HEAD])?)
        .trim()
        .to_owned();
    let named = std::env::var(SHA_VARIABLE).ok();
    if named.as_deref() != Some(head.as_str()) || expected.is_some_and(|sha| sha != head) {
        return Err(CiError::new(format!(
            "HEAD is not the commit of the event named by {SHA_VARIABLE}, so git cannot tell which files the event changes"
        )));
    }
    let dirty = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--ignore-submodules=none",
        ],
    )?;
    if !dirty.is_empty() {
        return Err(CiError::new(
            "tracked files differ from HEAD in the index or the worktree, so the commit diff does not describe the files being analysed",
        ));
    }
    Ok(())
}

pub(super) fn pull_request_base(root: &Path) -> Result<String, CiError> {
    git(root, &["rev-parse", "--verify", "--quiet", MERGED]).map_err(|_| {
        CiError::new("HEAD is not a merge commit, so git cannot tell which files the pull request changes; check out with fetch-depth: 2")
    })?;
    checked_out(root, None)?;
    Ok(BASE.to_owned())
}

pub(super) fn range_base(root: &Path, base: &str, head: &str) -> Result<String, CiError> {
    if !object_name(base) || !object_name(head) {
        return Err(CiError::new(format!(
            "the event names {base:?} and {head:?}, which are not both commit ids"
        )));
    }
    checked_out(root, Some(head))?;
    git(root, &["merge-base", "--is-ancestor", base, HEAD]).map_err(|_| {
        CiError::new(format!(
            "{base} is not an ancestor of {head} in this checkout, so git cannot tell which files the event changes"
        ))
    })?;
    Ok(base.to_owned())
}

fn object_name(text: &str) -> bool {
    matches!(text.len(), 40 | 64)
        && text.bytes().all(|b| b.is_ascii_hexdigit())
        && text.bytes().any(|b| b != b'0')
}

pub(super) fn changed_files(root: &Path, base: &str) -> Result<BTreeSet<RepoPath>, CiError> {
    let listing = git(
        root,
        &["diff", "--no-renames", "--name-only", "-z", base, HEAD],
    )?;
    String::from_utf8(listing)
        .map_err(|_| CiError::new("git listed a path that is not UTF-8"))?
        .split_terminator('\0')
        .map(|name| RepoPath::new(name).map_err(|error| CiError::new(format!("{name}: {error}"))))
        .collect()
}

pub(super) fn base_files(
    root: &Path,
    base: &str,
    wanted: &[RepoPath],
) -> BTreeMap<RepoPath, String> {
    let mut kept = BTreeMap::new();
    let mut bytes = 0;
    for path in wanted {
        let object = format!("{base}:{}", path.as_str());
        let Some(text) = git(root, &["show", &object])
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            continue;
        };
        bytes += text.len();
        if bytes > BASE_BYTES_CAP {
            break;
        }
        kept.insert(path.clone(), text);
    }
    kept
}
