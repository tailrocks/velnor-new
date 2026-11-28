// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::github::{ApiCompareFile, ApiCompareFiles, GitHub, Repo, compare_files_path, parse};
use crate::merge_group::commit;

const LOCK_FILE: &str = "Cargo.lock";
const PAGE_SIZE: u32 = 100;
const PAGES: u32 = 3;

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

/// Never fails: whatever it cannot establish is `Unknown`, which means "run everything".
#[must_use]
pub fn lock_change(github: &impl GitHub, repo: &Repo, before: &str, sha: &str) -> LockChange {
    if !commit(before) || zero(before) {
        return LockChange::Unknown("the push event names no before commit".to_owned());
    }
    for page in 1..=PAGES {
        let path = compare_files_path(repo, before, sha, page);
        let compare: ApiCompareFiles =
            match github.get(&path).and_then(|body| parse(&body, "compare")) {
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
        if compare.files.len() < PAGE_SIZE as usize {
            return LockChange::Unchanged;
        }
    }
    LockChange::Unknown(format!(
        "the compare lists {} files or more, so its file list may be truncated",
        PAGES * PAGE_SIZE
    ))
}
