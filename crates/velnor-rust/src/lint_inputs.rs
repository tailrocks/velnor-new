// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_model::{
    EventKind, JobId, LintJob, LintKind, PathGlob, RepoFacts, RepoPath, RootSlug, WORKFLOW_PATH,
};

use crate::config::CONFIG_PATH;
use crate::embedded::is_rust_source;
use crate::generator_inputs::Generator;
use crate::lint_tasks::{TASK_FOLDERS, task_names};
use crate::mise::{MISE_CONFIGS, MISE_LOCK};

#[must_use]
pub fn lint_runs(
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
    base: &BTreeMap<RepoPath, String>,
    unplain: &BTreeSet<RepoPath>,
) -> BTreeMap<RootSlug, bool> {
    let tree = Tree::new(facts, tracked, symlinks, base, unplain);
    facts
        .config
        .lint
        .iter()
        .filter(|lint| lint.planned())
        .map(|lint| (lint.slug(), runs(facts, lint, event, changed, &tree)))
        .collect()
}

struct Tree<'a> {
    tracked: &'a BTreeSet<RepoPath>,
    symlinks: &'a BTreeSet<RepoPath>,
    base: &'a BTreeMap<RepoPath, String>,
    unplain: &'a BTreeSet<RepoPath>,
    generator: Option<Generator>,
}

impl<'a> Tree<'a> {
    fn new(
        facts: &RepoFacts,
        tracked: &'a BTreeSet<RepoPath>,
        symlinks: &'a BTreeSet<RepoPath>,
        base: &'a BTreeMap<RepoPath, String>,
        unplain: &'a BTreeSet<RepoPath>,
    ) -> Self {
        let from_source = facts
            .config
            .lint
            .iter()
            .any(|lint| matches!(lint.kind, LintKind::GeneratedCheck { from_source: true }));
        Self {
            tracked,
            symlinks,
            base,
            unplain,
            generator: from_source.then(|| Generator::of(facts)),
        }
    }

    fn edited(&self, path: &RepoPath) -> bool {
        self.tracked.contains(path) && self.base.contains_key(path) && !self.symlinks.contains(path)
    }
}

pub fn task_files<'a>(
    facts: &'a RepoFacts,
    changed: &'a BTreeSet<RepoPath>,
) -> impl Iterator<Item = &'a RepoPath> {
    changed.iter().filter(|path| names_a_lint_task(facts, path))
}

fn runs(
    facts: &RepoFacts,
    lint: &LintJob,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    tree: &Tree<'_>,
) -> bool {
    let Some(changed) =
        changed.filter(|changed| event == EventKind::PullRequest && !changed.is_empty())
    else {
        return true;
    };
    triggered(facts, lint, changed, tree)
        || (!lint.inputs.is_empty() && !has_tracked_input(lint, tree.tracked, tree.symlinks))
}

fn triggered(
    facts: &RepoFacts,
    lint: &LintJob,
    changed: &BTreeSet<RepoPath>,
    tree: &Tree<'_>,
) -> bool {
    changed.iter().any(|path| reached(facts, lint, path, tree))
        || tree
            .symlinks
            .iter()
            .any(|symlink| linked(facts, lint, symlink, tree))
}

fn linked(facts: &RepoFacts, lint: &LintJob, symlink: &RepoPath, tree: &Tree<'_>) -> bool {
    match (&lint.kind, tree.generator.as_ref()) {
        (LintKind::GeneratedCheck { from_source: true }, Some(generator)) => {
            generator.reads(symlink)
        }
        _ => reached(facts, lint, symlink, tree),
    }
}

#[must_use]
pub fn unmatched_inputs(
    facts: &RepoFacts,
    event: EventKind,
    changed: Option<&BTreeSet<RepoPath>>,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
    base: &BTreeMap<RepoPath, String>,
    unplain: &BTreeSet<RepoPath>,
) -> Vec<(JobId, PathGlob)> {
    let changed = changed.filter(|_| event == EventKind::PullRequest);
    let tree = Tree::new(facts, tracked, symlinks, base, unplain);
    let entries = facts
        .config
        .lint
        .iter()
        .filter(|lint| !changed.is_some_and(|changed| triggered(facts, lint, changed, &tree)))
        .flat_map(|lint| {
            lint.inputs
                .iter()
                .map(|glob| (lint.id.clone(), glob.clone()))
        })
        .collect();
    unmatched_entries(entries, tracked, |glob, path| {
        input_matches(glob, path, symlinks)
    })
}

#[must_use]
pub fn unmatched_entries<T>(
    entries: Vec<(T, PathGlob)>,
    tracked: &BTreeSet<RepoPath>,
    matches: impl Fn(&PathGlob, &RepoPath) -> bool,
) -> Vec<(T, PathGlob)> {
    let mut by_head: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, (_, glob)) in entries.iter().enumerate() {
        by_head.entry(head(glob)).or_default().push(index);
    }
    let mut open = vec![true; entries.len()];
    let mut left = entries.len();
    for path in tracked {
        if left == 0 {
            break;
        }
        for key in [first_segment(path.as_str()), "*"] {
            for &index in by_head.get(key).into_iter().flatten() {
                if open[index] && matches(&entries[index].1, path) {
                    open[index] = false;
                    left -= 1;
                }
            }
        }
    }
    entries
        .into_iter()
        .zip(open)
        .filter_map(|(entry, open)| open.then_some(entry))
        .collect()
}

fn head(glob: &PathGlob) -> &str {
    first_segment(fixed(glob))
}

fn first_segment(text: &str) -> &str {
    text.split('/').next().unwrap_or("")
}

fn fixed(glob: &PathGlob) -> &str {
    let text = glob.as_str();
    text.strip_suffix("/**").unwrap_or(text)
}

fn input_matches(glob: &PathGlob, path: &RepoPath, symlinks: &BTreeSet<RepoPath>) -> bool {
    if symlinks.contains(path) {
        reaches(glob, path)
    } else {
        covers(glob, path)
    }
}

fn has_tracked_input(
    lint: &LintJob,
    tracked: &BTreeSet<RepoPath>,
    symlinks: &BTreeSet<RepoPath>,
) -> bool {
    tracked.iter().any(|path| {
        lint.inputs
            .iter()
            .any(|glob| input_matches(glob, path, symlinks))
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
        LintKind::Alint { config, probe, .. } => (Some(config), probe.as_ref()),
        LintKind::MiseTask { .. } | LintKind::GeneratedCheck { .. } => (None, None),
    };
    config.into_iter().chain(probe)
}

const GENERATOR_FOLDERS: [&str; 3] = [".github/", ".velnor/", "generator/"];

fn names_a_lint_task(facts: &RepoFacts, path: &RepoPath) -> bool {
    task_names(path).iter().any(|name| {
        facts
            .config
            .lint
            .iter()
            .any(|lint| matches!(&lint.kind, LintKind::MiseTask { task, .. } if task == name))
    })
}

fn in_generator_folder(path: &RepoPath) -> bool {
    GENERATOR_FOLDERS
        .iter()
        .any(|folder| path.as_str().starts_with(folder))
}

fn read_by_the_generator(facts: &RepoFacts, path: &RepoPath, tree: &Tree<'_>) -> bool {
    in_generator_folder(path) || (names_a_lint_task(facts, path) && !tree.edited(path))
}

fn is_markdown(path: &RepoPath) -> bool {
    Path::new(path.as_str())
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn inert(facts: &RepoFacts, path: &RepoPath, tree: &Tree<'_>) -> bool {
    !read_by_the_generator(facts, path, tree) && is_markdown(path)
}

fn generated_check_reached(
    from_source: bool,
    facts: &RepoFacts,
    path: &RepoPath,
    tree: &Tree<'_>,
) -> bool {
    let Some(generator) = tree.generator.as_ref().filter(|_| from_source) else {
        return from_source || !inert(facts, path, tree);
    };
    tree.unplain.contains(path)
        || in_generator_folder(path)
        || names_a_lint_task(facts, path)
        || generator.reads(path)
        || !(is_rust_source(path) || is_markdown(path))
}

fn declared(lint: &LintJob, path: &RepoPath) -> bool {
    lint.inputs.iter().any(|glob| reaches(glob, path))
}

fn reached(facts: &RepoFacts, lint: &LintJob, path: &RepoPath, tree: &Tree<'_>) -> bool {
    if let LintKind::GeneratedCheck { from_source } = lint.kind {
        return generated_check_reached(from_source, facts, path, tree);
    }
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
    let fixed = fixed(glob);
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
