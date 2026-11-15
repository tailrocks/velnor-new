// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use velnor_gen::io::{Checkout, Error as GenError, read_files, read_recorded_checkout};
use velnor_model::{AllTrigger, ChangeSet, RepoFacts, RepoPath, RustSelection};
use velnor_rust::{
    cargo_files, detect, optional_steps, policy_needed, root_sources, select, unknown_paths,
    unread_packages,
};

use super::{
    EVENT_VARIABLE, SCHEDULED, SOURCE_LIMIT, SOURCE_TOTAL_LIMIT, TOKEN_VARIABLE, api_url,
    append_outputs, current_dir, http,
};
use crate::changes::{base_files, changed_files};
use crate::cli::{BaseFile, PlanArgs};
use crate::error::CiError;
use crate::event::{Event, PullRequest, check_default_branch, parse_event};
use crate::github::Repo;
use crate::plan::{Steps, explain, outputs};

const EVENT_PATH_VARIABLE: &str = "GITHUB_EVENT_PATH";
const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const PULL_REQUEST: &str = "pull_request";
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
    let full = name.as_deref() == Some(SCHEDULED);
    let (selections, changes) = choose(args, &checkout, &facts, event.as_ref(), name.as_deref())?;
    let steps = facts
        .rust_roots
        .iter()
        .map(|root| {
            let steps = match root_sources(&facts, root, &checkout.view.tracked_paths) {
                None => optional_steps(root, &BTreeMap::new(), true),
                Some(paths) => match read_sources(&checkout.root, &paths) {
                    Ok(Some(sources)) => optional_steps(root, &sources, full),
                    Ok(None) | Err(GenError::NotRegular(_)) => {
                        optional_steps(root, &BTreeMap::new(), true)
                    }
                    Err(error) => return Err(ci(error)),
                },
            };
            Ok((root.manifest.clone(), steps))
        })
        .collect::<Result<Steps, CiError>>()?;
    let all_selected = selections.values().any(|selection| {
        matches!(selection, RustSelection::All(trigger) if !matches!(trigger, AllTrigger::MostPackages(_)))
    });
    let policy = policy_needed(&changes.paths, all_selected, facts.policy);
    let outputs = outputs(&selections, &steps, policy)?;
    append_outputs(&outputs)?;
    Ok(if args.explain {
        explain(
            &selections,
            &steps,
            &unknown_paths(&facts, &changes),
            &unread_packages(&facts, &changes),
        )
    } else {
        outputs
    })
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

fn choose(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    event: Option<&Event>,
    name: Option<&str>,
) -> Result<(BTreeMap<RepoPath, RustSelection>, ChangeSet), CiError> {
    if name == Some(SCHEDULED) {
        return Ok(everything(facts, &AllTrigger::Event));
    }
    let (paths, base_files) = if args.changed.is_empty() {
        if name != Some(PULL_REQUEST) {
            return Ok(everything(facts, &AllTrigger::Event));
        }
        let pull = event
            .and_then(|event| event.pull_request.as_ref())
            .ok_or_else(|| CiError::new("the pull_request event payload has no pull_request"))?;
        match pull_request_changes(checkout, facts, pull) {
            Ok(found) => found,
            Err(error) => {
                eprintln!("velnor-ci: {error}; every package is selected");
                return Ok(everything(facts, &AllTrigger::BaseUnavailable));
            }
        }
    } else {
        (
            args.changed.iter().cloned().collect(),
            read_base_files(&args.base_files)?,
        )
    };
    let changes = ChangeSet {
        base_files,
        head_files: read_head_files(checkout, &paths)?,
        paths,
    };
    Ok((select(facts, &changes), changes))
}

type Found = (BTreeSet<RepoPath>, BTreeMap<RepoPath, String>);

fn pull_request_changes(
    checkout: &Checkout,
    facts: &RepoFacts,
    pull: &PullRequest,
) -> Result<Found, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .map_err(|_| CiError::new(format!("{TOKEN_VARIABLE} is not set")))?;
    let repository = std::env::var(REPOSITORY_VARIABLE)
        .map_err(|_| CiError::new(format!("{REPOSITORY_VARIABLE} is not set")))?;
    let repo = Repo::parse(&repository)?;
    let github = http::HttpGitHub::new(api_url(), token, http::PLAN_TIMEOUT);
    let paths = changed_files(&github, &repo, pull)?.ok_or_else(|| {
        CiError::new("the pull request has too many files for the GitHub API to list")
    })?;
    let roots: BTreeSet<&RepoPath> = facts.rust_roots.iter().map(|root| &root.manifest).collect();
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| paths.contains(path) && (is_lock(path) || roots.contains(path)))
        .collect();
    let base = base_files(&github, &repo, pull, &wanted);
    Ok((paths, base))
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
