// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod changes;
mod claims;
mod config;
mod glob;
mod lex;
mod reads;
mod scan;
mod units;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::CiError;
use crate::escape::stderr_line;
use crate::write_request::internal;
use changes::{Changes, git, nul_paths};
use claims::Ownership;

const CRATE_JOB_PREFIX: &str = "rust-";
const CHECK_JOB_PREFIX: &str = "check-";
const SYMLINK_MODE: &str = "120000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Event {
    PullRequest,
    Fork,
    Local,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct LaneInventory {
    #[serde(default)]
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
struct Route {
    logical_job_id: String,
    job_id: String,
    #[serde(default)]
    task_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Input<'a> {
    pub(super) event: Event,
    pub(super) base: Option<&'a str>,
    pub(super) head: &'a str,
    pub(super) inventory: Option<&'a LaneInventory>,
}

#[derive(Debug, Default)]
struct Job {
    ids: BTreeSet<String>,
    packages: BTreeSet<String>,
    checks: BTreeSet<String>,
}

pub(super) fn not_affected_jobs(input: &Input<'_>, checkout: &Path) -> Result<String, CiError> {
    let root = toplevel(checkout)?;
    changes::verify_checkout(&root, input.event, input.head)
        .map_err(|problem| internal(&problem))?;
    let inventory = input
        .inventory
        .filter(|inventory| !inventory.routes.is_empty())
        .ok_or_else(|| internal("diff_selection_needs_lane_inventory"))?;
    let text = std::fs::read_to_string(root.join(config::PATH))
        .map_err(|_| internal("diff_selection_not_supported:config_unreadable"))?;
    let config = config::parse(&text)?;
    let mut jobs = jobs(inventory, &config)?;
    let package_dirs: BTreeSet<String> = jobs
        .values()
        .flat_map(|job| job.packages.iter().cloned())
        .collect();
    match select(&root, input, &config, &package_dirs) {
        Ok((packages, checks)) => {
            jobs.retain(|id, job| {
                skippable(id)
                    && job.packages.is_disjoint(&packages)
                    && (job.checks.is_empty()
                        || (input.event != Event::Local && job.checks.is_disjoint(&checks)))
            });
        }
        Err(reason) => {
            eprintln!("{}", stderr_line(&format!("{reason}:all_changed")));
            jobs.clear();
        }
    }
    let ids: BTreeSet<&str> = jobs
        .values()
        .flat_map(|job| job.ids.iter().map(String::as_str))
        .collect();
    Ok(if ids.is_empty() {
        String::new()
    } else {
        format!(",{},", ids.into_iter().collect::<Vec<_>>().join(","))
    })
}

fn skippable(logical: &str) -> bool {
    logical.starts_with(CRATE_JOB_PREFIX) || logical.starts_with(CHECK_JOB_PREFIX)
}

fn toplevel(checkout: &Path) -> Result<PathBuf, CiError> {
    let out = git(checkout, &["rev-parse", "--show-toplevel"])
        .map_err(|problem| internal(&format!("bad_root:{problem}")))?;
    let text = String::from_utf8(out).map_err(|_| internal("bad_root"))?;
    Ok(PathBuf::from(text.trim()))
}

fn jobs(
    inventory: &LaneInventory,
    config: &config::Config,
) -> Result<BTreeMap<String, Job>, CiError> {
    let mut jobs: BTreeMap<String, Job> = BTreeMap::new();
    for route in &inventory.routes {
        if !skippable(&route.logical_job_id) {
            continue;
        }
        let job = jobs.entry(route.logical_job_id.clone()).or_default();
        job.ids.insert(route.job_id.clone());
        for task in &route.task_ids {
            match task_owner(task) {
                Some(Owner::Package(dir)) => job.packages.insert(dir.to_owned()),
                Some(Owner::Check(id)) => job.checks.insert(id.to_owned()),
                None => {
                    return Err(internal(&format!(
                        "diff_selection_not_supported:task_id:{task}"
                    )));
                }
            };
        }
    }
    for check in &config.checks {
        let logical = format!("{CHECK_JOB_PREFIX}{}", check.id);
        let job = jobs.entry(logical.clone()).or_default();
        if job.ids.is_empty() {
            job.ids.insert(logical);
        }
        job.checks.insert(check.id.clone());
    }
    Ok(jobs)
}

enum Owner<'a> {
    Package(&'a str),
    Check(&'a str),
}

fn task_owner(task: &str) -> Option<Owner<'_>> {
    let (rest, check) = match task.strip_prefix("stack/rust/") {
        Some(rest) => (rest, false),
        None => (task.strip_prefix("stack/mise/")?, true),
    };
    let mut parts = rest.rsplitn(3, '/');
    let (_, _, owner) = (parts.next()?, parts.next()?, parts.next()?);
    if owner.is_empty() {
        return None;
    }
    Some(if check {
        Owner::Check(owner)
    } else {
        Owner::Package(owner)
    })
}

fn select(
    root: &Path,
    input: &Input<'_>,
    config: &config::Config,
    package_dirs: &BTreeSet<String>,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let changed = match changes::find(root, input.event, input.base, input.head) {
        Changes::Paths(paths) => paths,
        Changes::Broad(reason) => return Err(reason),
    };
    if let Some(reason) = changed.iter().find_map(|path| broadens(path)) {
        return Err(reason.to_owned());
    }
    let rust_changed: BTreeSet<String> = changed
        .iter()
        .filter(|path| !config.non_input(path))
        .cloned()
        .collect();
    if rust_changed.is_empty() {
        return Ok((BTreeSet::new(), BTreeSet::new()));
    }
    let tracked = tracked(root)?;
    let loaded = units::load(root, package_dirs, &tracked)?;
    let ownership = Ownership::new(&loaded.units, &config.rules)?;
    if claims::has_unowned_file(&ownership, &rust_changed) {
        return Err("unclassified_files".to_owned());
    }
    let forced = reads::forced_packages(root, &tracked, &loaded.units, &ownership);
    let base = base_of(root, input)?;
    let added = added_files(root, input, &base)?;
    let mut edges = loaded.edges.clone();
    edges.extend(units::base_edges(root, &base, &loaded.units, &added)?);
    let packages = claims::affected(&ownership, &rust_changed, &forced, &edges);
    let mut checks = BTreeSet::new();
    for check in &config.checks {
        if check.rust_changes
            || check_inputs(root, check)?
                .iter()
                .any(|path| changed.contains(path))
        {
            checks.insert(check.id.clone());
        }
    }
    Ok((packages, checks))
}

fn broadens(path: &str) -> Option<&'static str> {
    if path == "Cargo.lock" || path.ends_with("/Cargo.lock") {
        return Some("cargo_lock_changed");
    }
    if matches!(path, "Cargo.toml" | ".cargo/config.toml" | ".cargo/config") {
        return Some("root_config_changed");
    }
    if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
        return Some("outside_project_path:selecting_all");
    }
    if path == ".velnor" || path.starts_with(".velnor/") || path.starts_with(".github/") {
        return Some("global_config_changed:selecting_all");
    }
    None
}

fn tracked(root: &Path) -> Result<BTreeSet<String>, String> {
    let entries = git(root, &["ls-files", "-s", "-z"])?;
    let mut tracked = BTreeSet::new();
    for record in entries.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        let text = std::str::from_utf8(record).map_err(|_| "non_utf8_path".to_owned())?;
        let (meta, path) = text
            .split_once('\t')
            .ok_or_else(|| "malformed_index_entry".to_owned())?;
        if meta.split(' ').next() == Some(SYMLINK_MODE) {
            return Err("tracked_symlinks".to_owned());
        }
        tracked.insert(path.to_owned());
    }
    Ok(tracked)
}

fn base_of(root: &Path, input: &Input<'_>) -> Result<String, String> {
    if input.event == Event::Local {
        let head = git(root, &["rev-parse", "HEAD"])?;
        return Ok(String::from_utf8_lossy(&head).trim().to_owned());
    }
    input
        .base
        .map(str::to_owned)
        .ok_or_else(|| "comparison_unavailable:missing_base".to_owned())
}

fn added_files(root: &Path, input: &Input<'_>, base: &str) -> Result<BTreeSet<String>, String> {
    if input.event == Event::Local {
        return Ok(BTreeSet::new());
    }
    changes::sha(base, "bad_base")?;
    changes::sha(input.head, "bad_head")?;
    let range = format!("{base}...{}", input.head);
    nul_paths(&git(
        root,
        &[
            "diff",
            "-z",
            "--name-only",
            "--no-renames",
            "--diff-filter=A",
            &range,
            "--",
        ],
    )?)
}

fn check_inputs(root: &Path, check: &config::Check) -> Result<Vec<String>, String> {
    let prefix = if check.directory == "." {
        String::new()
    } else {
        format!("{}/", check.directory)
    };
    let sources: Vec<String> = ["mise.toml", ".mise.toml"]
        .iter()
        .map(|name| format!("{prefix}{name}"))
        .filter(|path| root.join(path).exists())
        .collect();
    let [source] = sources.as_slice() else {
        return Err(format!("mise_config:{}", check.id));
    };
    let mut inputs = check.inputs.clone();
    inputs.push(source.clone());
    Ok(inputs)
}
