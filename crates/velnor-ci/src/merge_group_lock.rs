// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::github::{ApiCompareFile, ApiCompareFiles, GitHub, Repo, compare_path, parse};
use crate::merge_group::commit;

const LOCK_FILE: &str = "Cargo.lock";
const LISTED_AT_MOST: usize = 300;

#[derive(Debug, PartialEq, Eq)]
pub enum LockChange {
    Unchanged,
    Changed(String),
    Unknown(String),
}

fn is_lock(path: &str) -> bool {
    path.rsplit('/').next() == Some(LOCK_FILE)
}

fn touched(file: &ApiCompareFile) -> Option<&str> {
    [
        Some(file.filename.as_str()),
        file.previous_filename.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|path| is_lock(path))
}

fn zero(sha: &str) -> bool {
    sha.bytes().all(|byte| byte == b'0')
}

#[must_use]
pub fn lock_change(github: &impl GitHub, repo: &Repo, before: &str, sha: &str) -> LockChange {
    if !commit(before) || zero(before) {
        return LockChange::Unknown("the push event names no before commit".to_owned());
    }
    let compare: ApiCompareFiles = match github
        .get(&compare_path(repo, before, sha))
        .and_then(|body| parse(&body, "compare"))
    {
        Ok(compare) => compare,
        Err(error) => return LockChange::Unknown(error.to_string()),
    };
    if compare.status != "ahead" {
        return LockChange::Unknown(format!(
            "the push is {} its before commit, not ahead of it",
            compare.status
        ));
    }
    if let Some(path) = compare.files.iter().find_map(touched) {
        return LockChange::Changed(path.to_owned());
    }
    if compare.files.len() >= LISTED_AT_MOST {
        return LockChange::Unknown(format!(
            "the compare lists {LISTED_AT_MOST} files or more, so its file list may be truncated"
        ));
    }
    LockChange::Unchanged
}
