// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{
    ApiCompare, GitHub, Repo, commit_pulls_path, compare_path, parse, pull_commits_path,
};

const PAGE: usize = 100;

#[derive(Debug, Deserialize)]
struct ApiPull {
    number: u64,
    head: ApiHead,
}

#[derive(Debug, Deserialize)]
struct ApiHead {
    #[serde(rename = "ref")]
    branch: String,
}

#[derive(Debug, Deserialize)]
struct ApiPrCommit {
    sha: String,
}

pub fn check_release_source(
    github: &dyn GitHub,
    repo: &Repo,
    verified: &str,
    prefixes: &[String],
) -> Result<(), CiError> {
    if verified.len() != 40
        || !verified
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CiError::new(format!(
            "the verified commit `{}` is not a full lowercase commit SHA",
            verified.escape_debug()
        )));
    }
    let pulls: Vec<ApiPull> = parse(
        &github.get(&commit_pulls_path(repo, verified))?,
        "pull requests of the commit",
    )?;
    if pulls.len() >= PAGE {
        return Err(CiError::new(format!(
            "{PAGE} or more pull requests are associated with {verified}; cannot tell which release pull request release-plz would tag"
        )));
    }
    for pull in pulls.iter().filter(|pull| {
        prefixes
            .iter()
            .any(|p| pull.head.branch.starts_with(p.as_str()))
    }) {
        let commits: Vec<ApiPrCommit> = parse(
            &github.get(&pull_commits_path(repo, pull.number))?,
            "commits of the pull request",
        )?;
        let Some(chosen) = commits.last() else {
            continue;
        };
        if chosen.sha == verified {
            continue;
        }
        let compare: ApiCompare = parse(
            &github.get(&compare_path(repo, &chosen.sha, verified))?,
            "comparison",
        )?;
        if compare.status == "ahead" {
            return Err(CiError::new(format!(
                "release pull request #{} has its last commit {} inside the verified commit {verified}, so release-plz would tag that commit and not the verified one; release PRs must be squash-merged (or merged through a merge queue with squash) so the tag names the verified commit",
                pull.number, chosen.sha
            )));
        }
    }
    Ok(())
}
