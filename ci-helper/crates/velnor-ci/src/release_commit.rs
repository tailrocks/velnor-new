// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiCompare, GitHub, Repo, compare_path, parse, tag_object_path, tag_ref_path};
use crate::release::PlainName;

const COMMIT_SUFFIX: &str = "_commit";

#[derive(Debug, Deserialize)]
struct ApiTarget {
    object: ApiObject,
}

#[derive(Debug, Deserialize)]
struct ApiObject {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

pub fn tagged_commits(
    github: &dyn GitHub,
    repo: &Repo,
    verified: &str,
    tags: &BTreeMap<PlainName, PlainName>,
) -> Result<String, CiError> {
    let verified = full_sha("the verified commit", verified)?;
    let mut lines = String::new();
    for (package, tag) in tags {
        let output = format!("{package}{COMMIT_SUFFIX}");
        if tags.keys().any(|other| other.as_str() == output) {
            return Err(CiError::new(format!(
                "the commit output of {package} would be named {output}, which is also a released package"
            )));
        }
        let commit = tagged_commit(github, repo, tag)?;
        require_in_history(github, repo, tag, &commit, &verified)?;
        lines.push_str(&[output.as_str(), "=", commit.as_str(), "\n"].concat());
    }
    Ok(lines)
}

fn tagged_commit(github: &dyn GitHub, repo: &Repo, tag: &PlainName) -> Result<String, CiError> {
    let target: ApiTarget = parse(&github.get(&tag_ref_path(repo, tag.as_str()))?, "tag")?;
    let ApiObject { kind, sha } = target.object;
    match kind.as_str() {
        "commit" => full_sha(&format!("the commit of tag {tag}"), &sha),
        "tag" => {
            let inner: ApiTarget = parse(&github.get(&tag_object_path(repo, &sha))?, "tag object")?;
            match inner.object.kind.as_str() {
                "commit" => full_sha(&format!("the commit of tag {tag}"), &inner.object.sha),
                "tag" => Err(CiError::new(format!(
                    "tag {tag} points at a tag that points at a tag, not at a commit"
                ))),
                other => Err(not_a_commit(tag, other)),
            }
        }
        other => Err(not_a_commit(tag, other)),
    }
}

fn not_a_commit(tag: &PlainName, kind: &str) -> CiError {
    CiError::new(format!("tag {tag} names a {kind}, not a commit"))
}

fn require_in_history(
    github: &dyn GitHub,
    repo: &Repo,
    tag: &PlainName,
    commit: &str,
    verified: &str,
) -> Result<(), CiError> {
    let compare: ApiCompare = parse(
        &github.get(&compare_path(repo, commit, verified))?,
        "comparison",
    )?;
    if matches!(compare.status.as_str(), "identical" | "ahead") {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "tag {tag} is on {commit}, which is not in the history of the verified commit {verified} (comparison: {})",
            compare.status
        )))
    }
}

fn full_sha(what: &str, sha: &str) -> Result<String, CiError> {
    if sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(sha.to_owned())
    } else {
        Err(CiError::new(format!(
            "{what} `{}` is not a full lowercase commit SHA",
            sha.escape_debug()
        )))
    }
}
