// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    EventKind, LintJob, LintKind, PathGlob, RepoFacts, RepoPath, RootSlug, WORKFLOW_PATH,
};

use crate::config::CONFIG_PATH;
use crate::lint_tasks::TASK_FOLDERS;
use crate::mise::{MISE_CONFIGS, MISE_LOCK};

#[must_use]
pub fn lint_runs(
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
) -> BTreeMap<RootSlug, bool> {
    facts
        .config
        .lint
        .iter()
        .filter(|lint| !lint.inputs.is_empty())
        .map(|lint| (lint.slug(), runs(lint, event, changed, tracked, symlinks)))
        .collect()
}

fn runs(
    lint: &LintJob,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
) -> bool {
    let Some(changed) = changed.filter(|_| event == EventKind::PullRequest) else {
        return true;
    };
    symlinks
        .iter()
        .chain(changed)
        .any(|path| reached(lint, path))
        || !has_tracked_input(lint, tracked, symlinks)
}

fn has_tracked_input(
    lint: &LintJob,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
) -> bool {
    tracked.iter().any(|path| {
        lint.inputs.iter().any(|glob| {
            if symlinks.contains(path) {
                reaches(glob, path)
            } else {
                covers(glob, path)
            }
        })
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

fn declared(lint: &LintJob, path: &RepoPath) -> bool {
    lint.inputs.iter().any(|glob| reaches(glob, path))
}

fn reached(lint: &LintJob, path: &RepoPath) -> bool {
    declared(lint, path)
        || implicit(lint, path)
        || below_one(lint, path)
        || (matches!(lint.kind, LintKind::MiseTask { .. })
            && TASK_FOLDERS.iter().any(|folder| {
                path.as_str().starts_with(folder) || folder.starts_with(&format!("{path}/"))
            }))
}

fn below_one(lint: &LintJob, path: &RepoPath) -> bool {
    [WORKFLOW_PATH, CONFIG_PATH, MISE_LOCK]
        .iter()
        .chain(&MISE_CONFIGS)
        .chain(own_files(lint))
        .any(|own| own.as_str().starts_with(&format!("{path}/")))
}

fn reaches(glob: &PathGlob, path: &RepoPath) -> bool {
    matches(glob, path, true)
}

fn covers(glob: &PathGlob, path: &RepoPath) -> bool {
    matches(glob, path, false)
}

fn matches(glob: &PathGlob, path: &RepoPath, ancestors: bool) -> bool {
    let text = glob.as_str();
    let trailing = text.ends_with("/**");
    let fixed = if trailing {
        &text[..text.len() - "/**".len()]
    } else {
        text
    };
    let open = trailing || !text.contains('*');
    let (want, got) = (depth(fixed), depth(path.as_str()));
    let agrees = fixed
        .split('/')
        .zip(path.as_str().split('/'))
        .all(|(want, got)| want == got || want == "*");
    agrees
        && match got.cmp(&want) {
            Ordering::Less => ancestors,
            Ordering::Equal => ancestors || !trailing,
            Ordering::Greater => open,
        }
}

fn depth(path: &str) -> usize {
    path.matches('/').count() + 1
}
