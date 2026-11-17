// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use super::{Error, program};

pub(super) fn repository_name(root: &Path) -> Result<String, Error> {
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
    match remote.as_deref() {
        None => Ok(String::new()),
        Some(url) => repository_path(url).ok_or_else(|| {
            Error::Repository("origin must identify a hosted repository; local or invalid origins cannot determine its schedule".to_owned())
        }),
    }
}

fn repository_path(url: &str) -> Option<String> {
    let remote = url.trim().trim_end_matches('/');
    let path = if let Some((scheme, path)) = remote.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return None;
        }
        let (host, path) = path.split_once('/')?;
        if host.is_empty() {
            return None;
        }
        path
    } else {
        let (host, path) = remote.split_once(':')?;
        if host.is_empty() || host.contains('/') || host.contains('\\') {
            return None;
        }
        path
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
