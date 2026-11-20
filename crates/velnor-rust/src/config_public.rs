// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{GitRepository, PublicSources};

use crate::error::Reason;
use crate::public_git::slug;

const GITHUB: &str = "https://github.com/";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPublicSources {
    git: Vec<String>,
}

pub(crate) fn optional(raw: Option<RawPublicSources>) -> Result<Option<PublicSources>, Reason> {
    raw.map(|raw| public_sources(&raw)).transpose()
}

fn public_sources(raw: &RawPublicSources) -> Result<PublicSources, Reason> {
    let invalid = |value: &str| Reason::InvalidValue {
        key: "public_sources.git",
        value: value.to_owned(),
    };
    if raw.git.is_empty() {
        return Err(invalid("[]"));
    }
    let mut git: Vec<GitRepository> = Vec::new();
    for entry in &raw.git {
        let (owner, name) = entry
            .strip_prefix(GITHUB)
            .and_then(|path| path.split_once('/'))
            .filter(|(owner, name)| {
                slug(owner) && slug(name) && name.strip_suffix(".git").is_none()
            })
            .ok_or_else(|| invalid(entry))?;
        if git.iter().any(|known| known.matches(owner, name)) {
            return Err(invalid(entry));
        }
        git.push(GitRepository {
            owner: owner.to_owned(),
            name: name.to_owned(),
        });
    }
    Ok(PublicSources { git })
}
