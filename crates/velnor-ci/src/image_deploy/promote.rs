// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, Repo, parse};

pub trait Registry {
    fn digest(&self, reference: &str) -> Result<Option<String>, CiError>;
    fn tag(&self, source: &str, target: &str) -> Result<(), CiError>;
}

pub struct Promotion<'a> {
    pub images: &'a [String],
    pub version: &'a str,
    pub commit: &'a str,
    pub live_commit: &'a dyn Fn() -> Result<String, CiError>,
}

impl std::fmt::Debug for Promotion<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Promotion")
            .field("images", &self.images)
            .field("version", &self.version)
            .field("commit", &self.commit)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RefObject {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

#[derive(Deserialize)]
struct RefAnswer {
    object: RefObject,
}

#[derive(Deserialize)]
struct TagAnswer {
    object: RefObject,
}

pub fn live_tag_commit(github: &dyn GitHub, repo: &Repo, tag: &str) -> Result<String, CiError> {
    let answer: RefAnswer = parse(
        &github.get(&format!("/repos/{repo}/git/ref/tags/{tag}"))?,
        "tag reference",
    )?;
    let mut object = answer.object;
    for _ in 0..4 {
        match object.kind.as_str() {
            "commit" => return Ok(object.sha),
            "tag" => {
                let inner: TagAnswer = parse(
                    &github.get(&format!("/repos/{repo}/git/tags/{}", object.sha))?,
                    "tag object",
                )?;
                object = inner.object;
            }
            other => {
                return Err(CiError::new(format!(
                    "the tag {tag} names a {other}, not a commit"
                )));
            }
        }
    }
    Err(CiError::new(format!("the tag {tag} is nested too deeply")))
}

#[must_use]
pub fn is_missing(reference: &str, stderr: &str) -> bool {
    let miss = format!("{reference}: not found").to_ascii_lowercase();
    stderr.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.contains("manifest unknown") || (line.starts_with("error: ") && line.ends_with(&miss))
    })
}

enum Plan {
    Create { digest: String },
    Present { digest: String },
}

fn number(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn image(text: &str) -> bool {
    text.contains('/')
        && !text.starts_with(['/', '.', '-'])
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"./_-".contains(&byte)
        })
}

fn validate(promotion: &Promotion<'_>) -> Result<(), CiError> {
    let Promotion {
        images,
        version,
        commit,
        ..
    } = promotion;
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|part| number(part)) {
        return Err(CiError::new(format!("`{version}` is not a version X.Y.Z")));
    }
    if commit.len() != 40
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CiError::new(format!(
            "`{commit}` is not a full lowercase commit SHA"
        )));
    }
    if images.is_empty() {
        return Err(CiError::new("name at least one --image"));
    }
    for (index, name) in images.iter().enumerate() {
        if !image(name) || images[..index].contains(name) {
            return Err(CiError::new(format!(
                "`{name}` is not a distinct registry repository like ghcr.io/owner/name"
            )));
        }
    }
    Ok(())
}

fn plan(registry: &dyn Registry, name: &str, promotion: &Promotion<'_>) -> Result<Plan, CiError> {
    let source = format!("{name}:sha-{}", promotion.commit);
    let wanted = registry.digest(&source)?.ok_or_else(|| {
        CiError::new(format!(
            "{source} does not exist because a superseded commit gets no image, so open the push workflow run of commit {}, choose Re-run all jobs, then re-run this job",
            promotion.commit
        ))
    })?;
    let target = format!("{name}:{}", promotion.version);
    match registry.digest(&target)? {
        None => Ok(Plan::Create { digest: wanted }),
        Some(existing) if existing == wanted => Ok(Plan::Present { digest: wanted }),
        Some(existing) => Err(CiError::new(format!(
            "{target} already names {existing}, and {source} is {wanted}; a released version is never moved to another image, so cut a new version"
        ))),
    }
}

pub fn promote(registry: &dyn Registry, promotion: &Promotion<'_>) -> Result<String, CiError> {
    validate(promotion)?;
    let mut plans = Vec::new();
    for name in promotion.images {
        plans.push((name, plan(registry, name, promotion)?));
    }
    let digests: Vec<&String> = plans
        .iter()
        .map(|(_, plan)| match plan {
            Plan::Create { digest } | Plan::Present { digest } => digest,
        })
        .collect();
    if digests.windows(2).any(|pair| pair[0] != pair[1]) {
        let listed = plans
            .iter()
            .map(|(name, plan)| match plan {
                Plan::Create { digest } | Plan::Present { digest } => format!("{name}: {digest}"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CiError::new(format!(
            "the registries name different images for sha-{}: {listed}; a publish re-run changed one of them, so nothing was tagged; re-run the publish of that commit until both agree, or cut a new version",
            promotion.commit
        )));
    }
    let live = (promotion.live_commit)()?;
    if live != promotion.commit {
        return Err(CiError::new(format!(
            "the release tag now names commit {live} and not {}; it moved while the run waited, so nothing was tagged",
            promotion.commit
        )));
    }
    let mut report = String::new();
    for (name, plan) in plans {
        let target = format!("{name}:{}", promotion.version);
        match plan {
            Plan::Create { digest } => {
                registry.tag(&format!("{name}@{digest}"), &target)?;
                let now = registry.digest(&target)?;
                if now.as_deref() != Some(digest.as_str()) {
                    return Err(CiError::new(format!(
                        "{target} was tagged with {digest} and now names {}; another writer moved it, so check the registry before anything else is promoted",
                        now.as_deref().unwrap_or("nothing")
                    )));
                }
                let _ = writeln!(report, "{target} now names {digest}");
            }
            Plan::Present { digest } => {
                let _ = writeln!(report, "{target} already names {digest}");
            }
        }
    }
    Ok(report)
}
