// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiCompare, GitHub, Repo, compare_path, parse};
use crate::release::PlainName;
use crate::release_commit::tagged_commit;

const VERSION: &str = "{{ version }}";
const PAGE: usize = 100;

pub trait GitHubPost: GitHub {
    fn post(&self, path: &str, json: &str) -> Result<String, CiError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Family {
    Build { tag: String, commit: String },
    Released { tag: String, commit: Option<String> },
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
}

#[derive(Debug, Deserialize)]
struct ApiRef {
    #[serde(rename = "ref")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
}

#[must_use]
pub fn lines(outcome: &Family) -> String {
    match outcome {
        Family::Build { tag, commit } => format!("tag={tag}\ncommit={commit}\nbuild=true\n"),
        Family::Released {
            tag,
            commit: Some(commit),
        } => format!("tag={tag}\ncommit={commit}\nbuild=false\n"),
        Family::Released { tag, commit: None } => format!("tag={tag}\nbuild=false\n"),
    }
}

pub fn version_of(metadata: &[String], packages: &[String]) -> Result<String, CiError> {
    let mut found = BTreeSet::new();
    for package in packages {
        let mut versions = Vec::new();
        for text in metadata {
            let parsed: Metadata = serde_json::from_str(text)
                .map_err(|e| CiError::new(format!("not the output of cargo metadata: {e}")))?;
            versions.extend(
                parsed
                    .packages
                    .into_iter()
                    .filter(|candidate| candidate.name == *package)
                    .map(|candidate| candidate.version),
            );
        }
        match versions.as_slice() {
            [only] => {
                found.insert(only.clone());
            }
            _ => {
                return Err(CiError::new(format!(
                    "package {package} must appear once in the cargo metadata, and appears {} times",
                    versions.len()
                )));
            }
        }
    }
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (Some(version), None) => Ok(version),
        _ => Err(CiError::new(
            "the binaries of the family do not share one version",
        )),
    }
}

pub fn tag_of(pattern: &str, version: &str) -> Result<PlainName, CiError> {
    if pattern.matches(VERSION).count() != 1 {
        return Err(CiError::new(format!(
            "tag pattern `{}` must hold {VERSION} once",
            pattern.escape_debug()
        )));
    }
    PlainName::tag(&pattern.replace(VERSION, version))
}

pub fn require_pushed(tag: &PlainName, pushed: &str) -> Result<(), CiError> {
    if tag.as_str() == pushed {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "the pushed tag {} is not the family tag {} of the package versions",
            pushed.escape_debug(),
            tag.as_str()
        )))
    }
}

fn releases(github: &dyn GitHub, repo: &Repo, tag: &str) -> Result<Vec<ApiRelease>, CiError> {
    let mut found = Vec::new();
    for page in 1.. {
        let items: Vec<ApiRelease> = parse(
            &github.get(&format!(
                "/repos/{repo}/releases?per_page={PAGE}&page={page}"
            ))?,
            "releases",
        )?;
        let last = items.len() < PAGE;
        found.extend(items.into_iter().filter(|release| release.tag_name == tag));
        if last {
            break;
        }
    }
    Ok(found)
}

fn tag_exists(github: &dyn GitHub, repo: &Repo, tag: &str) -> Result<bool, CiError> {
    let refs: Vec<ApiRef> = parse(
        &github.get(&format!(
            "/repos/{repo}/git/matching-refs/tags/{tag}?per_page={PAGE}"
        ))?,
        "matching tags",
    )?;
    Ok(refs
        .iter()
        .any(|found| found.name == format!("refs/tags/{tag}")))
}

pub fn family_tag(
    github: &dyn GitHubPost,
    repo: &Repo,
    verified: &str,
    tag: &PlainName,
) -> Result<Family, CiError> {
    let verified = crate::release_commit::full_sha("the verified commit", verified)?;
    let tag = tag.as_str();
    let published = releases(github, repo, tag)?
        .iter()
        .filter(|release| !release.draft)
        .count();
    if !tag_exists(github, repo, tag)? {
        if published != 0 {
            return Err(CiError::new(format!(
                "release {tag} is published but its tag is missing"
            )));
        }
        github.post(
            &format!("/repos/{repo}/git/refs"),
            &serde_json::json!({"ref": format!("refs/tags/{tag}"), "sha": verified}).to_string(),
        )?;
    }
    let name = PlainName::tag(tag)?;
    let target = tagged_commit(github, repo, &name)?;
    if target != verified {
        let relation: ApiCompare = parse(
            &github.get(&compare_path(repo, &target, &verified))?,
            "comparison",
        )?;
        if published != 0 && relation.status == "ahead" {
            return Ok(Family::Released {
                tag: tag.to_owned(),
                commit: None,
            });
        }
        return Err(CiError::new(format!(
            "tag {tag} is on {target}, not on the verified commit {verified}"
        )));
    }
    if published != 0 {
        return Ok(Family::Released {
            tag: tag.to_owned(),
            commit: Some(verified),
        });
    }
    if releases(github, repo, tag)?.is_empty() {
        github.post(
            &format!("/repos/{repo}/releases"),
            &serde_json::json!({
                "tag_name": tag,
                "target_commitish": verified,
                "name": tag,
                "body": format!("Built from {verified}."),
                "draft": true,
            })
            .to_string(),
        )?;
    }
    Ok(Family::Build {
        tag: tag.to_owned(),
        commit: verified,
    })
}
