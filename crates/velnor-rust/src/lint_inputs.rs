// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    EventKind, LintJob, LintKind, PathGlob, RepoFacts, RepoPath, RootSlug, WORKFLOW_PATH,
};

use crate::config::{CONFIG_PATH, rule_matches};
use crate::lint_tasks::TASK_FOLDERS;
use crate::mise::{MISE_CONFIGS, MISE_LOCK};

#[must_use]
pub fn lint_runs(
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    symlinks: &BTreeSet<RepoPath>,
) -> BTreeMap<RootSlug, bool> {
    facts
        .config
        .lint
        .iter()
        .filter(|lint| !lint.inputs.is_empty())
        .map(|lint| (lint.slug(), runs(lint, event, changed, symlinks)))
        .collect()
}

fn runs(
    lint: &LintJob,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    symlinks: &BTreeSet<RepoPath>,
) -> bool {
    let Some(changed) = changed.filter(|_| event == EventKind::PullRequest) else {
        return true;
    };
    symlinks.iter().any(|link| linked(lint, link))
        || changed.iter().any(|path| {
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
        LintKind::MiseTask { .. } | LintKind::GeneratedCheck => (None, None),
    };
    config.into_iter().chain(probe)
}

fn task_file(lint: &LintJob, path: &RepoPath) -> bool {
    matches!(lint.kind, LintKind::MiseTask { .. })
        && TASK_FOLDERS
            .iter()
            .any(|folder| path.as_str().starts_with(folder))
}

fn linked(lint: &LintJob, link: &RepoPath) -> bool {
    lint.inputs.iter().any(|glob| reaches(glob, link))
        || implicit(lint, link)
        || [WORKFLOW_PATH, CONFIG_PATH, MISE_LOCK]
            .iter()
            .chain(&MISE_CONFIGS)
            .chain(own_files(lint))
            .any(|own| own.as_str().starts_with(&format!("{link}/")))
        || (matches!(lint.kind, LintKind::MiseTask { .. })
            && TASK_FOLDERS.iter().any(|folder| {
                link.as_str().starts_with(folder) || folder.starts_with(&format!("{link}/"))
            }))
}

fn reaches(glob: &PathGlob, link: &RepoPath) -> bool {
    let pattern: Vec<&str> = glob.as_str().split('/').collect();
    let segments: Vec<&str> = link.as_str().split('/').collect();
    let open = pattern.len() > 1 && pattern.last() == Some(&"**");
    let fixed = if open {
        &pattern[..pattern.len() - 1]
    } else {
        &pattern[..]
    };
    let shared = fixed.len().min(segments.len());
    let agrees = fixed[..shared]
        .iter()
        .zip(&segments)
        .all(|(want, got)| want == got || *want == "*");
    agrees && (segments.len() <= fixed.len() || open)
}
