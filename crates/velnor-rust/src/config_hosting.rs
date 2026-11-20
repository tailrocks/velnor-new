// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use velnor_model::{CratesMirror, HostedRunners, RunnerLabel, TestGroup, looks_github_hosted};

use crate::error::Reason;

/// Why the `hosted` declarations of `.velnor/rust-ci.toml` are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostingError {
    /// A label spelled like a GitHub-hosted one, with no `hosted` key.
    Undeclared(String),
    /// One label declared hosted in one place and self-hosted in another.
    Conflict(String),
    /// A test group with `hosted` but no `runner`.
    NoRunner(String),
}

impl fmt::Display for HostingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undeclared(label) => write!(
                f,
                "runner {label} looks GitHub-hosted; declare `hosted = true` (open to fork pull requests) or `hosted = false` (fenced) in rust-ci.toml"
            ),
            Self::Conflict(label) => write!(
                f,
                "runner {label} is declared both hosted and self-hosted; one label has one `hosted` value"
            ),
            Self::NoRunner(group) => write!(f, "test group {group} sets `hosted` without `runner`"),
        }
    }
}

/// Decides which labels are open to fork pull requests. Only an explicit `hosted = true`
/// opens one, so an unknown, custom or undeclared label stays fenced. An undeclared label
/// that is spelled like a GitHub-hosted one is refused: the repository must say which it is.
/// The default `runner` (key absent) is GitHub's own `ubuntu-latest` and counts as declared.
pub(crate) fn resolve(
    runner: (&RunnerLabel, Option<bool>),
    mirror: Option<&CratesMirror>,
    groups: &[TestGroup],
) -> Result<HostedRunners, Reason> {
    let mut uses = vec![runner];
    uses.extend(mirror.map(|mirror| (&mirror.runner, Some(false))));
    for group in groups {
        match (&group.runner, group.hosted) {
            (Some(label), declared) => uses.push((label, declared)),
            (None, Some(_)) => {
                return Err(Reason::Hosting(HostingError::NoRunner(
                    group.name.to_string(),
                )));
            }
            (None, None) => {}
        }
    }
    let mut declared: BTreeMap<String, (bool, &RunnerLabel)> = BTreeMap::new();
    for (label, hosted) in uses
        .iter()
        .filter_map(|(label, hosted)| Some((*label, (*hosted)?)))
    {
        let key = label.as_str().to_ascii_lowercase();
        if declared
            .insert(key.clone(), (hosted, label))
            .is_some_and(|(was, _)| was != hosted)
        {
            return Err(Reason::Hosting(HostingError::Conflict(label.to_string())));
        }
    }
    if let Some((label, _)) = uses.iter().find(|(label, hosted)| {
        hosted.is_none()
            && looks_github_hosted(label.as_str())
            && !declared.contains_key(&label.as_str().to_ascii_lowercase())
    }) {
        return Err(Reason::Hosting(HostingError::Undeclared(label.to_string())));
    }
    Ok(HostedRunners::new(
        declared
            .into_values()
            .filter(|(hosted, _)| *hosted)
            .map(|(_, label)| label.clone())
            .collect(),
    ))
}

pub(crate) fn hosted_flags(
    declared_runner: bool,
    hosted: Option<bool>,
    test_groups: &[TestGroup],
) -> (bool, bool) {
    let hosted_key = hosted.is_some() || test_groups.iter().any(|group| group.hosted.is_some());
    (declared_runner, hosted_key)
}
