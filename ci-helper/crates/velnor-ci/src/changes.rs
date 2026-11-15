// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::RepoPath;

use crate::error::CiError;
use crate::event::PullRequest;
use crate::github::{ApiPullFile, GitHub, Repo, contents_path, parse, pull_files_path};

pub const FILE_LIST_CAP: usize = 3_000;
pub const BASE_READ_CAP: usize = 64;
pub const BASE_BYTES_CAP: usize = 4 << 20;
const PAGE_SIZE: usize = 100;
const MAX_PAGES: u32 = 30;

const _: () = assert!(MAX_PAGES as usize * PAGE_SIZE == FILE_LIST_CAP);

pub fn changed_files(
    github: &dyn GitHub,
    repo: &Repo,
    pull: &PullRequest,
) -> Result<Option<BTreeSet<RepoPath>>, CiError> {
    let mut paths = BTreeSet::new();
    let mut seen = 0;
    for page in 1..=MAX_PAGES {
        let body = github.get(&pull_files_path(repo, pull.number, page))?;
        let files: Vec<ApiPullFile> = parse(&body, "pull request files")?;
        seen += files.len();
        for file in files {
            for name in std::iter::once(file.filename).chain(file.previous_filename) {
                paths.insert(
                    RepoPath::new(name.as_str())
                        .map_err(|error| CiError::new(format!("{name}: {error}")))?,
                );
            }
        }
        if seen < page as usize * PAGE_SIZE {
            return Ok(Some(paths));
        }
    }
    Ok(None)
}

#[must_use]
pub fn base_files(
    github: &dyn GitHub,
    repo: &Repo,
    pull: &PullRequest,
    wanted: &[RepoPath],
) -> BTreeMap<RepoPath, String> {
    let mut kept = BTreeMap::new();
    let mut bytes = 0;
    for path in wanted.iter().take(BASE_READ_CAP) {
        let Ok(text) = github.get_raw(&contents_path(repo, path.as_str(), &pull.base)) else {
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
