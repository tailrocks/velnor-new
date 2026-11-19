// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use velnor_gen::io::{Checkout, Error as GenError, read_files, read_recorded_checkout};
use velnor_model::{
    AllTrigger, ChangeSet, EventKind, RepoFacts, RepoPath, RustRoot, RustSelection,
};
use velnor_rust::{
    cargo_files, detect, introduced, member_sources, optional_steps, policy_runs, root_sources,
    select_event, selects_by_diff, tests_expected, unknown_paths, unread_packages,
};

use super::{
    EVENT_VARIABLE, SCHEDULED, SOURCE_LIMIT, SOURCE_TOTAL_LIMIT, SUMMARY_VARIABLE, append_outputs,
    append_to, current_dir, git,
};
use crate::cli::{BaseFile, PlanArgs};
use crate::error::CiError;
use crate::escape::stderr_line;
use crate::event::{Event, check_default_branch, parse_event};
use crate::plan::{Steps, explain, outputs, tests_outputs, warnings, weakened};
use crate::summary::summary;

const EVENT_PATH_VARIABLE: &str = "GITHUB_EVENT_PATH";
const LOCK_FILE: &str = "Cargo.lock";

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

pub(super) fn run_plan(args: &PlanArgs) -> Result<String, CiError> {
    let checkout = read_recorded_checkout(&current_dir()?).map_err(ci)?;
    let facts = detect(&checkout.view).map_err(ci)?;
    let event = read_event()?;
    if let Some(event) = &event {
        check_default_branch(&facts.config.default_branch, event)?;
    }
    let name = std::env::var(EVENT_VARIABLE).ok();
    let full = args.full || name.as_deref() == Some(SCHEDULED);
    let (selections, changes) = choose(args, &checkout, &facts, name.as_deref(), event.as_ref())?;
    let steps = facts
        .rust_roots
        .iter()
        .map(|whole| {
            let selection = selections.get(&whole.manifest);
            if matches!(selection, Some(RustSelection::None)) {
                return Ok((
                    whole.manifest.clone(),
                    optional_steps(whole, &BTreeMap::new(), true),
                ));
            }
            let root = &scoped(whole, selection);
            let found = if matches!(selection, Some(RustSelection::Some(_))) {
                member_sources(&facts, root, &checkout.view.tracked_paths)
            } else {
                root_sources(&facts, root, &checkout.view.tracked_paths)
            };
            let steps = match found {
                None => optional_steps(root, &BTreeMap::new(), true),
                Some(paths) => match read_sources(&checkout.root, &paths) {
                    Ok(Some(sources)) => optional_steps(root, &sources, full),
                    Ok(None) | Err(GenError::NotRegular(_)) => {
                        optional_steps(root, &BTreeMap::new(), true)
                    }
                    Err(error) => return Err(ci(error)),
                },
            };
            Ok((whole.manifest.clone(), steps))
        })
        .collect::<Result<Steps, CiError>>()?;
    let everything = selections.values().any(|selection| {
        matches!(selection, RustSelection::All(trigger) if !matches!(trigger, AllTrigger::MostPackages(_)))
    });
    let selected = selections
        .values()
        .any(|selection| !matches!(selection, RustSelection::None));
    let policy = policy_runs(facts.policy, &changes.paths, everything, selected);
    let expected: BTreeMap<RepoPath, bool> = facts
        .rust_roots
        .iter()
        .map(|root| {
            let tests = selections
                .get(&root.manifest)
                .is_some_and(|selection| tests_expected(&facts, root, selection));
            (root.manifest.clone(), tests)
        })
        .collect();
    let outputs = outputs(&selections, &steps, policy)? + &tests_outputs(&expected);
    append_outputs(&outputs)?;
    append_to(
        SUMMARY_VARIABLE,
        &summary(&facts.rust_roots, &selections, &steps),
    )?;
    let new = introduced(&facts, &changes);
    let report = if args.explain {
        explain(
            &selections,
            &steps,
            &unknown_paths(&facts, &changes),
            &unread_packages(&facts, &changes),
        ) + &weakened(&facts.weakenings, &new)
    } else {
        outputs
    };
    Ok(report + &warnings(&new))
}

fn event_kind(name: Option<&str>, manual: bool) -> EventKind {
    match name {
        Some("pull_request") => EventKind::PullRequest,
        Some("push") => EventKind::Push,
        Some("merge_group") => EventKind::MergeGroup,
        Some(SCHEDULED) => EventKind::Schedule,
        Some("workflow_dispatch") => EventKind::WorkflowDispatch,
        _ if manual => EventKind::PullRequest,
        _ => EventKind::WorkflowDispatch,
    }
}

fn choose(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    name: Option<&str>,
    event: Option<&Event>,
) -> Result<(BTreeMap<RepoPath, RustSelection>, ChangeSet), CiError> {
    let kind = if args.full {
        EventKind::WorkflowDispatch
    } else {
        event_kind(name, !args.changed.is_empty())
    };
    let diff = if selects_by_diff(kind, &facts.config) {
        changes(args, checkout, facts, kind, event)?
    } else {
        None
    };
    Ok(select_event(facts, kind, || diff))
}

fn changes(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Option<ChangeSet>, CiError> {
    let (paths, base_files) = if args.changed.is_empty() {
        match event_changes(checkout, facts, kind, event) {
            Ok(found) => found,
            Err(error) => {
                eprintln!(
                    "{}",
                    stderr_line(&format!("{error}; every package is selected"))
                );
                return Ok(None);
            }
        }
    } else {
        (
            args.changed.iter().cloned().collect(),
            read_base_files(&args.base_files)?,
        )
    };
    Ok(Some(ChangeSet {
        base_files,
        head_files: read_head_files(checkout, &paths)?,
        paths,
    }))
}

type Found = (BTreeSet<RepoPath>, BTreeMap<RepoPath, String>);

fn event_changes(
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Found, CiError> {
    let base = if kind == EventKind::PullRequest {
        git::pull_request_base(&checkout.root)?
    } else {
        let range = event
            .and_then(|event| event.range.as_ref())
            .ok_or_else(|| CiError::new("the event payload names no commit range"))?;
        git::range_base(&checkout.root, &range.base, &range.head)?
    };
    let paths = git::changed_files(&checkout.root, &base)?;
    let roots: BTreeSet<&RepoPath> = facts.rust_roots.iter().map(|root| &root.manifest).collect();
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| paths.contains(path) && (is_lock(path) || roots.contains(path)))
        .chain(
            facts
                .weakenings
                .iter()
                .map(|weakening| &weakening.file)
                .filter(|file| paths.contains(file))
                .cloned(),
        )
        .collect::<BTreeSet<RepoPath>>()
        .into_iter()
        .collect();
    let files = git::base_files(&checkout.root, &base, &wanted);
    Ok((paths, files))
}

fn scoped(root: &RustRoot, selection: Option<&RustSelection>) -> RustRoot {
    match selection {
        Some(RustSelection::Some(packages)) => RustRoot {
            members: root
                .members
                .iter()
                .filter(|member| packages.contains_key(&member.name))
                .cloned()
                .collect(),
            ..root.clone()
        },
        _ => root.clone(),
    }
}

fn is_lock(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(LOCK_FILE)
}

fn read_event() -> Result<Option<Event>, CiError> {
    let Some(file) = std::env::var_os(EVENT_PATH_VARIABLE).filter(|file| !file.is_empty()) else {
        return Ok(None);
    };
    let text = fs::read_to_string(&file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", Path::new(&file).display())))?;
    parse_event(&text).map(Some)
}

fn read_sources(
    root: &Path,
    paths: &[RepoPath],
) -> Result<Option<BTreeMap<RepoPath, Vec<u8>>>, GenError> {
    let mut total = 0_u64;
    let oversized = paths.iter().any(|path| {
        let size =
            fs::symlink_metadata(root.join(path.as_str())).map_or(0, |metadata| metadata.len());
        total = total.saturating_add(size);
        size > SOURCE_LIMIT || total > SOURCE_TOTAL_LIMIT
    });
    if oversized {
        return Ok(None);
    }
    read_files(root, paths).map(Some)
}

fn read_base_files(files: &[BaseFile]) -> Result<BTreeMap<RepoPath, String>, CiError> {
    files
        .iter()
        .map(|base| {
            fs::read_to_string(&base.file)
                .map(|text| (base.path.clone(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", base.file.display())))
        })
        .collect()
}

fn read_head_files(
    checkout: &Checkout,
    changed: &BTreeSet<RepoPath>,
) -> Result<BTreeMap<RepoPath, String>, CiError> {
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| changed.contains(path))
        .collect();
    let files = read_files(&checkout.root, &wanted).map_err(ci)?;
    Ok(files
        .into_iter()
        .filter_map(|(path, bytes)| Some((path, String::from_utf8(bytes).ok()?)))
        .collect())
}
