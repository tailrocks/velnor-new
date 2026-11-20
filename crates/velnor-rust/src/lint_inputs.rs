// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{EventKind, LintJob, LintKind, RepoFacts, RepoPath, RootSlug, WORKFLOW_PATH};

use crate::config::{CONFIG_PATH, rule_matches};

/// Whether each lint that declares `inputs` runs.
///
/// Only a pull request with a known list of changed files can skip a lint: every other event,
/// and a pull request whose changed files could not be listed (`changed` is `None`), runs it.
#[must_use]
pub fn lint_runs(
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
) -> BTreeMap<RootSlug, bool> {
    facts
        .config
        .lint
        .iter()
        .filter(|lint| !lint.inputs.is_empty())
        .map(|lint| (lint.slug(), runs(lint, event, changed)))
        .collect()
}

fn runs(lint: &LintJob, event: EventKind, changed: Option<&BTreeSet<RepoPath>>) -> bool {
    let Some(changed) = changed.filter(|_| event == EventKind::PullRequest) else {
        return true;
    };
    changed.iter().any(|path| {
        lint.inputs.iter().any(|glob| rule_matches(glob, path))
            || [&WORKFLOW_PATH, &CONFIG_PATH]
                .into_iter()
                .chain(own_files(lint))
                .any(|own| own == path)
    })
}

fn own_files(lint: &LintJob) -> impl Iterator<Item = &RepoPath> {
    let (config, probe) = match &lint.kind {
        LintKind::Jactionlint { config } => (config.as_ref(), None),
        LintKind::Alint { config, probe } => (Some(config), probe.as_ref()),
        LintKind::MiseTask { .. } => (None, None),
    };
    config.into_iter().chain(probe)
}
