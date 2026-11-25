// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiRepo, GitHub, Repo, branch_path, parse, repo_path};
use crate::image_deploy::promote::Registry;

#[derive(Debug)]
pub struct Advancement<'a> {
    pub images: &'a [String],
    pub branch: &'a str,
    pub commit: &'a str,
    pub digest: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advanced {
    pub moved: bool,
    pub report: String,
}

#[derive(Deserialize)]
struct Tip {
    commit: TipCommit,
}

#[derive(Deserialize)]
struct TipCommit {
    sha: String,
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate(advancement: &Advancement<'_>) -> Result<(), CiError> {
    let Advancement {
        images,
        branch,
        commit,
        digest,
    } = advancement;
    if !hex(commit, 40) {
        return Err(CiError::new(format!(
            "`{commit}` is not a full lowercase commit SHA"
        )));
    }
    if !digest
        .strip_prefix("sha256:")
        .is_some_and(|rest| hex(rest, 64))
    {
        return Err(CiError::new(format!("`{digest}` is not a sha256 digest")));
    }
    if branch.is_empty() || branch.starts_with(['.', '-']) || branch.contains([':', '@', '/']) {
        return Err(CiError::new(format!("`{branch}` cannot be an image tag")));
    }
    if images.is_empty() {
        return Err(CiError::new("name at least one --image"));
    }
    Ok(())
}

fn tip_commit(github: &dyn GitHub, repo: &Repo) -> Result<String, CiError> {
    let ApiRepo { default_branch, .. } = parse(&github.get(&repo_path(repo))?, "repository")?;
    let tip: Tip = parse(&github.get(&branch_path(repo, &default_branch))?, "branch")?;
    Ok(tip.commit.sha)
}

fn targets(
    registry: &dyn Registry,
    advancement: &Advancement<'_>,
    tip: &str,
) -> Result<Result<Vec<(String, String)>, String>, CiError> {
    let mut wanted = Vec::new();
    for name in advancement.images {
        if tip == advancement.commit {
            wanted.push((name.clone(), advancement.digest.to_owned()));
            continue;
        }
        match registry.digest(&format!("{name}:sha-{tip}"))? {
            Some(digest) => wanted.push((name.clone(), digest)),
            None => {
                return Ok(Err(format!(
                    "{name}:sha-{tip} does not exist, so the tip has no image to point {} at; nothing moved\n",
                    advancement.branch
                )));
            }
        }
    }
    Ok(Ok(wanted))
}

pub fn advance(
    github: &dyn GitHub,
    repo: &Repo,
    registry: &dyn Registry,
    advancement: &Advancement<'_>,
) -> Result<Advanced, CiError> {
    validate(advancement)?;
    let tip = tip_commit(github, repo)?;
    let wanted = match targets(registry, advancement, &tip)? {
        Ok(wanted) => wanted,
        Err(report) => {
            return Ok(Advanced {
                moved: false,
                report,
            });
        }
    };
    let mut report = String::new();
    if tip != advancement.commit {
        let _ = writeln!(
            report,
            "commit {} is no longer the tip, so {} follows the tip {tip}",
            advancement.commit, advancement.branch
        );
    }
    let mut failures = Vec::new();
    for (name, digest) in &wanted {
        let target = format!("{name}:{}", advancement.branch);
        if registry.digest(&target)?.as_deref() == Some(digest.as_str()) {
            let _ = writeln!(report, "{target} already names {digest}");
            continue;
        }
        let now = tip_commit(github, repo)?;
        if now != tip {
            return Err(CiError::new(format!(
                "the tip of the default branch moved from {tip} to {now} while {} was being moved; nothing more was written, and the advance job of the new tip points the tag at it (re-run this job when that one failed)",
                advancement.branch
            )));
        }
        if let Err(error) = registry.tag(&format!("{name}@{digest}"), &target) {
            failures.push(format!("{target} could not be created: {error}"));
        }
    }
    for (name, digest) in &wanted {
        let target = format!("{name}:{}", advancement.branch);
        match registry.digest(&target) {
            Ok(Some(now)) if now == *digest => {
                let _ = writeln!(report, "{target} names {digest}");
            }
            Ok(now) => failures.push(format!(
                "{target} names {} and not {digest}",
                now.as_deref().unwrap_or("nothing")
            )),
            Err(error) => failures.push(format!("{target} cannot be read back: {error}")),
        }
    }
    if failures.is_empty() {
        Ok(Advanced {
            moved: true,
            report,
        })
    } else {
        Err(CiError::new(format!(
            "{}\nthe registries disagree about {}; re-run this job to repair them",
            failures.join("\n"),
            advancement.branch
        )))
    }
}
