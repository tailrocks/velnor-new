// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use velnor_model::RepoPath;

use crate::error::CiError;

const BASE: &str = "HEAD^1";
const HEAD: &str = "HEAD";
const MERGED: &str = "HEAD^2";

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

pub(super) fn changed_files(root: &Path) -> Result<BTreeSet<RepoPath>, CiError> {
    git(root, &["rev-parse", "--verify", "--quiet", MERGED]).map_err(|_| {
        CiError::new("HEAD is not a merge commit, so git cannot tell which files the pull request changes; check out with fetch-depth: 2")
    })?;
    let listing = git(
        root,
        &["diff", "--no-renames", "--name-only", "-z", BASE, HEAD],
    )?;
    String::from_utf8(listing)
        .map_err(|_| CiError::new("git listed a path that is not UTF-8"))?
        .split_terminator('\0')
        .map(|name| RepoPath::new(name).map_err(|error| CiError::new(format!("{name}: {error}"))))
        .collect()
}

pub(super) fn base_files(root: &Path, wanted: &[RepoPath]) -> BTreeMap<RepoPath, String> {
    wanted
        .iter()
        .filter_map(|path| {
            let object = format!("{BASE}:{}", path.as_str());
            let bytes = git(root, &["show", &object]).ok()?;
            Some((path.clone(), String::from_utf8(bytes).ok()?))
        })
        .collect()
}
