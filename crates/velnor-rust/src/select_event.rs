// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::select::select;
use velnor_model::{
    AllTrigger, ChangeSet, EventKind, OnMain, RepoFacts, RepoPath, RustCiConfig, RustSelection,
};

#[must_use]
pub fn selects_by_diff(event: EventKind, config: &RustCiConfig) -> bool {
    match event {
        EventKind::PullRequest => true,
        EventKind::Push | EventKind::MergeGroup => config.on_main == OnMain::Affected,
        EventKind::Schedule | EventKind::WorkflowDispatch => false,
    }
}

pub fn select_event(
    facts: &RepoFacts,
    event: EventKind,
    diff: impl FnOnce() -> Option<ChangeSet>,
) -> (BTreeMap<RepoPath, RustSelection>, ChangeSet) {
    if !selects_by_diff(event, &facts.config) {
        return everything(facts, &AllTrigger::Event);
    }
    match diff() {
        Some(changes) => (select(facts, &changes), changes),
        None => everything(facts, &AllTrigger::BaseUnavailable),
    }
}

fn everything(
    facts: &RepoFacts,
    trigger: &AllTrigger,
) -> (BTreeMap<RepoPath, RustSelection>, ChangeSet) {
    let selections = facts
        .rust_roots
        .iter()
        .map(|root| (root.manifest.clone(), RustSelection::All(trigger.clone())))
        .collect();
    (selections, ChangeSet::default())
}
