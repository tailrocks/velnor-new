// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{EventKind, JobId, RepoFacts, RepoPath, RustSelection, TestGroup};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRun {
    pub run: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlan {
    pub root: RepoPath,
    pub name: JobId,
    pub decision: GroupRun,
}

fn decided(run: bool, reason: impl Into<String>) -> GroupRun {
    GroupRun {
        run,
        reason: reason.into(),
    }
}

#[must_use]
pub fn group_runs(
    group: &TestGroup,
    event: EventKind,
    changed: &BTreeSet<RepoPath>,
    selection: &RustSelection,
) -> GroupRun {
    if matches!(selection, RustSelection::None) {
        return decided(false, "no package is selected, so no archive is built");
    }
    if event == EventKind::Push {
        return decided(true, "a push to the default branch runs every group");
    }
    if group.paths.is_empty() && group.events.is_empty() {
        return decided(
            true,
            "paths and events are not set, so the group runs with its root",
        );
    }
    if group.events.iter().any(|listed| listed.kind() == event) {
        return decided(true, format!("event {} is listed in events", event.name()));
    }
    if event == EventKind::MergeGroup && group.paths.is_empty() {
        return decided(true, "events never skip a group on merge_group");
    }
    let matched = changed.iter().find_map(|path| {
        group
            .paths
            .iter()
            .find(|glob| glob.matches(path))
            .map(|glob| (path, glob))
    });
    if let Some((path, glob)) = matched {
        return decided(true, format!("changed path {path} matches paths {glob}"));
    }
    if matches!(selection, RustSelection::All(_)) {
        return decided(true, "every package is selected");
    }
    decided(
        false,
        format!(
            "event {} is not listed in events and no changed path matches paths",
            event.name()
        ),
    )
}

#[must_use]
pub fn plan_groups(
    facts: &RepoFacts,
    event: EventKind,
    changed: &BTreeSet<RepoPath>,
    selections: &BTreeMap<RepoPath, RustSelection>,
) -> Vec<GroupPlan> {
    let count = facts.rust_roots.len();
    facts
        .rust_roots
        .iter()
        .flat_map(|root| {
            let selection = selections
                .get(&root.manifest)
                .unwrap_or(&RustSelection::None);
            crate::config_test_groups::of_root(&facts.config, root, count).map(move |group| {
                GroupPlan {
                    root: root.manifest.clone(),
                    name: group.name.clone(),
                    decision: group_runs(group, event, changed, selection),
                }
            })
        })
        .collect()
}
