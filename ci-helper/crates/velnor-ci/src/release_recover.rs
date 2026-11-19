// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::CiError;
use crate::github::{GitHub, Repo, parse, release_by_tag_path, tag_ref_path};
use crate::publish::{ApiRelease, author, made_by, pages};
use crate::release::{PlainName, output_name};

const PAGE: usize = 100;

pub fn recover(
    github: &dyn GitHub,
    repo: &Repo,
    app: &PlainName,
    expected: &BTreeMap<String, String>,
    recorded: &BTreeMap<PlainName, PlainName>,
) -> Result<BTreeMap<PlainName, PlainName>, CiError> {
    let bot = format!("{app}[bot]");
    let mut releases: Option<Vec<ApiRelease>> = None;
    let mut found = BTreeMap::new();
    for (package, tag) in expected {
        let (package, tag) = (output_name(package)?, PlainName::tag(tag)?);
        if recorded.contains_key(&package) || recorded.values().any(|other| *other == tag) {
            continue;
        }
        if absent(github, &tag_ref_path(repo, tag.as_str()))? || published(github, repo, &tag)? {
            continue;
        }
        if releases.is_none() {
            releases = Some(pages(github, "releases", |page| {
                format!("/repos/{repo}/releases?per_page={PAGE}&page={page}")
            })?);
        }
        let tagged: Vec<&ApiRelease> = releases
            .iter()
            .flatten()
            .filter(|release| release.tag_name == tag.as_str())
            .collect();
        match tagged.as_slice() {
            [] => {}
            [only] if !only.draft => {}
            [only] if only.draft && made_by(only, &bot) => {
                found.insert(package, tag);
            }
            [only] if only.draft => {
                return Err(CiError::new(format!(
                    "draft release {tag} ({}) was created by {}, not by {bot}",
                    only.id,
                    author(only)
                )));
            }
            many => {
                let listed: Vec<String> = many
                    .iter()
                    .map(|release| format!("{} by {}", release.id, author(release)))
                    .collect();
                return Err(CiError::new(format!(
                    "{} draft releases have the tag {tag} ({}); only one, created by {bot}, may",
                    many.len(),
                    listed.join(", ")
                )));
            }
        }
    }
    Ok(found)
}

fn absent(github: &dyn GitHub, path: &str) -> Result<bool, CiError> {
    match github.get(path) {
        Ok(_) => Ok(false),
        Err(error) if error.is_missing() => Ok(true),
        Err(error) => Err(error),
    }
}

fn published(github: &dyn GitHub, repo: &Repo, tag: &PlainName) -> Result<bool, CiError> {
    match github.get(&release_by_tag_path(repo, tag.as_str())) {
        Ok(body) => Ok(!parse::<ApiRelease>(&body, "release")?.draft),
        Err(error) if error.is_missing() => Ok(false),
        Err(error) => Err(error),
    }
}
