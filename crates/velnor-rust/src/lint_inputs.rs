// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{EventKind, LintJob, LintKind, RepoFacts, RepoPath, RootSlug, WORKFLOW_PATH};

use crate::config::{CONFIG_PATH, rule_matches};
use crate::lint_tasks::TASK_FOLDERS;
use crate::mise::{MISE_CONFIGS, MISE_LOCK};

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
            || implicit(lint, path)
            || task_file(lint, path)
    })
}

fn implicit(lint: &LintJob, path: &RepoPath) -> bool {
    [WORKFLOW_PATH, CONFIG_PATH, MISE_LOCK]
        .iter()
        .chain(&MISE_CONFIGS)
        .chain(own_files(lint))
        .any(|own| own == path)
}

fn own_files(lint: &LintJob) -> impl Iterator<Item = &RepoPath> {
    let (config, probe) = match &lint.kind {
        LintKind::Jactionlint { config } => (config.as_ref(), None),
        LintKind::Alint { config, probe } => (Some(config), probe.as_ref()),
        LintKind::MiseTask { .. } => (None, None),
    };
    config.into_iter().chain(probe)
}

fn task_file(lint: &LintJob, path: &RepoPath) -> bool {
    matches!(lint.kind, LintKind::MiseTask { .. })
        && TASK_FOLDERS
            .iter()
            .any(|folder| path.as_str().starts_with(folder))
}
