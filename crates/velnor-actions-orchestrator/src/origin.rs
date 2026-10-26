// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

pub(crate) const GITHUB_REPOSITORY_ENV: &str = "GITHUB_REPOSITORY";

pub(crate) fn origin_url_via_git(root: &Path) -> Option<String> {
    let output = GitRequest::config(vec![
        OsString::from("--get"),
        OsString::from("remote.origin.url"),
    ])
    .run_in(root)
    .ok()?;
    if !output.success {
        return None;
    }
    let url = output.stdout_text("git").ok()?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

pub(crate) fn validate_repository_slug(raw: &str) -> Option<String> {
    if raw.chars().any(char::is_whitespace) {
        return None;
    }
    let mut parts = raw.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{}/{}", owner.to_lowercase(), repo.to_lowercase()))
}
