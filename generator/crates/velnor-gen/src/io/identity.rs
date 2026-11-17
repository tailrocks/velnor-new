// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use super::program;

pub(super) fn repository_name(root: &Path) -> String {
    let remote = program(
        root,
        "git",
        &[
            OsStr::new("config"),
            OsStr::new("--get"),
            OsStr::new("remote.origin.url"),
        ],
    )
    .ok()
    .and_then(|bytes| String::from_utf8(bytes).ok());
    remote
        .as_deref()
        .and_then(repository_path)
        .unwrap_or_default()
}

fn repository_path(url: &str) -> Option<String> {
    let remote = url.trim().trim_end_matches('/');
    let path = if let Some((_, path)) = remote.split_once("://") {
        path.split_once('/')?.1
    } else if let Some((_, path)) = remote.split_once(':') {
        path
    } else {
        remote
    };
    let parts: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let owner = *parts.get(parts.len().checked_sub(2)?)?;
    let repo = parts.last()?.strip_suffix(".git").unwrap_or(parts.last()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}").to_ascii_lowercase())
}
