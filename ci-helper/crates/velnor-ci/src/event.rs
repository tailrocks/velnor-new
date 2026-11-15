// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_rust::CONFIG_PATH;

use crate::error::CiError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub default_branch: Option<String>,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub base: String,
    pub head: String,
}

#[derive(Deserialize)]
struct Payload {
    repository: Option<Repository>,
    before: Option<String>,
    after: Option<String>,
    merge_group: Option<MergeGroup>,
}

#[derive(Deserialize)]
struct MergeGroup {
    base_sha: Option<String>,
    head_sha: Option<String>,
}

#[derive(Deserialize)]
struct Repository {
    default_branch: Option<String>,
}

pub fn parse_event(text: &str) -> Result<Event, CiError> {
    let payload: Payload = serde_json::from_str(text)
        .map_err(|error| CiError::new(format!("the event payload is not readable: {error}")))?;
    let range = match payload.merge_group {
        Some(group) => group.base_sha.zip(group.head_sha),
        None => payload.before.zip(payload.after),
    };
    Ok(Event {
        default_branch: payload.repository.and_then(|repo| repo.default_branch),
        range: range.map(|(base, head)| Range { base, head }),
    })
}

pub fn check_default_branch(configured: &str, event: &Event) -> Result<(), CiError> {
    let actual = event.default_branch.as_deref().ok_or_else(|| {
        CiError::new(
            "the event payload has no repository.default_branch to check default_branch against",
        )
    })?;
    if actual == configured {
        return Ok(());
    }
    Err(CiError::new(format!(
        "default_branch is \"{configured}\" in {CONFIG_PATH}, but the default branch of this repository is \"{actual}\"; with a wrong name no run writes the cache and merges are not checked. Set default_branch = \"{actual}\" in {CONFIG_PATH} and run velnor-gen generate"
    )))
}
