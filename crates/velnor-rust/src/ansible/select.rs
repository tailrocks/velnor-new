// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{EventKind, RepoFacts, RepoPath, RepoView, WORKFLOW_PATH};

use super::{LAYOUT, playbooks, slug_collision};
use crate::config::CONFIG_PATH;
use crate::error::DetectError;

pub fn ansible_runs(
    view: &RepoView,
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    unknown: &BTreeSet<RepoPath>,
) -> Result<Option<bool>, DetectError> {
    if playbooks(view)?.is_none() {
        return Ok(None);
    }
    slug_collision(facts)?;
    Ok(Some(ansible_affected(view, event, changed, unknown)))
}

fn ansible_affected(
    view: &RepoView,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    unknown: &BTreeSet<RepoPath>,
) -> bool {
    if event != EventKind::PullRequest {
        return true;
    }
    let Some(changed) = changed else {
        return true;
    };
    if changed.is_empty() {
        return true;
    }
    changed
        .iter()
        .chain(view.symlinks.iter())
        .any(|path| affects_ansible(path, unknown))
}

fn affects_ansible(path: &RepoPath, unknown: &BTreeSet<RepoPath>) -> bool {
    let text = path.as_str();
    text == "ansible-configs"
        || text.starts_with(LAYOUT)
        || path == &WORKFLOW_PATH
        || path == &CONFIG_PATH
        || unknown.contains(path)
}
