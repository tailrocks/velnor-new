// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;


use crate::OrchestratorError;
use crate::origin::origin_url_via_git;

pub(crate) fn origin_repository(root: &Path) -> Result<String, OrchestratorError> {
    let Some(url) = origin_url_via_git(root) else {
        return Err(OrchestratorError::IdentityRejected {
            problem: "release_origin_unresolvable:set remote origin to the repository URL"
                .to_owned(),
        });
    };
    normalize_origin(&url).ok_or_else(|| OrchestratorError::IdentityRejected {
        problem: "release_origin_not_github:origin must be a github.com owner/repo URL".to_owned(),
    })
}

fn normalize_origin(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some((host, path)) = crate::prepare::split_host_path(base) {
        if host.eq_ignore_ascii_case("github.com") && is_owner_repo(path) {
            return Some(path.to_owned());
        }
        return None;
    }
    is_owner_repo(base).then(|| base.to_owned())
}

fn is_owner_repo(path: &str) -> bool {
    let Some((owner, name)) = path.split_once('/') else {
        return false;
    };
    !name.contains('/') && is_repo_segment(owner) && is_repo_segment(name)
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
