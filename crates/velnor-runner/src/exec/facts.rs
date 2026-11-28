// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::context_value::ContextValue;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobFacts {
    pub event_name: String,
    pub git_ref: String,
    pub base_ref: String,
    pub pull_request: Option<u64>,
    pub default_branch: Option<String>,
    pub cache_mode: Option<String>,
}

fn text(root: &ContextValue, path: &[&str]) -> Option<String> {
    let node = path.iter().try_fold(root, |node, key| node.get(key))?;
    match node {
        ContextValue::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn pull_request_of(git_ref: &str) -> Option<u64> {
    git_ref
        .strip_prefix("refs/pull/")?
        .strip_suffix("/merge")?
        .parse()
        .ok()
}

impl JobFacts {
    pub(super) fn new(github: &ContextValue, cache_mode: Option<&str>) -> Self {
        let top = |name: &str| text(github, &[name]).unwrap_or_default();
        let git_ref = top("ref");
        Self {
            event_name: top("event_name"),
            base_ref: top("base_ref"),
            pull_request: pull_request_of(&git_ref),
            git_ref,
            default_branch: text(github, &["event", "repository", "default_branch"])
                .filter(|value| !value.is_empty()),
            cache_mode: cache_mode.map(ToOwned::to_owned),
        }
    }
}
