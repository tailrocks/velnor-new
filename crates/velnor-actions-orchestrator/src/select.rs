//! Obligation universe and affected-work classification for `plan-v1`.
//!
//! The plan carries the full configured obligation universe. On events
//! that select affected work (pull requests), packages the change cannot
//! reach are classified unchanged and their crate jobs become
//! `not_affected`; any doubt (unknown files, git failures, lockfile
//! changes, unresolved graphs, unbounded symlinks) selects everything.
//! Full-run events (push, merge group, qualification) change everything.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{ProposedTask, WorkflowEvent};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::SelectionBroadening;

use crate::decisions::{broadening_for_path, selection_broadens_for_path};
use crate::discover::Discovery;
use crate::git_paths::{NON_UTF8_PATH, split_nul_paths};
use crate::select_affected::{Ownership, affected_packages, has_unowned_file};
use crate::select_edges::{base_edges, head_edges};
use crate::validators::{validate_diff_rev, validate_select_diff_args};

#[path = "select_checkout.rs"]
mod checkout;
#[path = "select_mode.rs"]
mod mode;
#[path = "select_paths.rs"]
mod paths;
pub(crate) use checkout::{verify_checkout, verify_checkout_until};
pub(crate) use mode::{SelectionMode, selection_mode};
use mode::{event_token, resolved_edges, resolved_links};

use paths::{
    PROJECT_TOOL_CONFIG_CHANGED, head_sha, is_advisory_toolfile, is_project_tool_source,
    tree_diff_names, untracked_files,
};

/// Full obligation universe: every task with applicable targets.
///
/// Tasks without applicable targets are never obligations: scheduling
/// them would emit impossible work (for example `cargo test --doc` for a
/// package with no doctest-able target). Each omission is recorded as a
/// `valid_no_test_targets:<task-id>` warning, never silent. Tofu
/// subdir roots additionally record one `path.cwd:<root>` caveat each.
pub(crate) fn select_universe<'a>(
    discovery: &'a Discovery,
    warnings: &mut Vec<String>,
) -> Result<Vec<&'a ProposedTask>, crate::OrchestratorError> {
    let mut kept = Vec::new();
    for task in &discovery.proposals {
        if task.no_targets {
            warnings.push(format!("valid_no_test_targets:{}", task.task_id));
        } else {
            kept.push(task);
        }
    }
    crate::select_tofu::push_chdir_findings(discovery, warnings)?;
    Ok(kept)
}

/// Changed package IDs, or `None` when the comparison is unknown.
///
/// `None` marks every obligation changed (fail-open broad: the correct
/// unknown-comparison behavior). An empty set means nothing changed, so
/// obligations stay eligible for verified coverage instead of executing
/// unconditionally. Push compares `before...head` when a base exists.
pub(crate) fn classify_changed(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &Discovery,
    warnings: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    if selection_mode(event) == SelectionMode::Full {
        warnings.push(format!("full_run_event:{}:all_changed", event_token(event)));
        return None;
    }
    if discovery.skipped_non_utf8 {
        warnings.push(format!(
            "comparison_unavailable:{NON_UTF8_PATH}:all_changed"
        ));
        return None;
    }
    if event == WorkflowEvent::Local {
        return classify_local(root, discovery, warnings);
    }
    let Some(base) = base else {
        warnings.push("comparison_unavailable:missing_base:all_changed".to_owned());
        return None;
    };
    let (changed, toolfiles) = change_sets(root, base, head, warnings)?;
    Some(affected_from_changed(
        discovery, &changed, toolfiles, root, base, head, warnings,
    ))
}

/// Local pre-push classification: the working tree against `HEAD`.
fn classify_local(
    root: &Path,
    discovery: &Discovery,
    warnings: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    let sha = match head_sha(root) {
        Ok(sha) => sha,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    let (changed, toolfiles) = local_change_set(root, warnings)?;
    Some(affected_from_changed(
        discovery, &changed, toolfiles, root, &sha, &sha, warnings,
    ))
}

/// Changed package IDs for one change set, broadening on risk.
///
/// An empty change set marks nothing changed: obligations stay eligible
/// for verified coverage, and execute when no proof covers them. Any
/// risk (lockfile, root config, broadening paths, unowned files, edge
/// failures) marks every package changed.
fn affected_from_changed(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    toolfiles: bool,
    root: &Path,
    base: &str,
    head: &str,
    warnings: &mut Vec<String>,
) -> BTreeSet<String> {
    let tasks = &discovery.proposals;
    let all_packages: BTreeSet<String> = tasks
        .iter()
        .map(|task| task.identity.unit_id.clone())
        .collect();
    if changed.is_empty() {
        warnings.push(
            if toolfiles {
                "toolfiles_only:all_unchanged"
            } else {
                "no_affected_files:all_unchanged"
            }
            .to_owned(),
        );
        return BTreeSet::new();
    }
    if changed
        .iter()
        .any(|path| broadening_for_path(path) == Some(SelectionBroadening::Lockfile))
    {
        warnings.push("cargo_lock_changed:all_changed".to_owned());
        return all_packages;
    }
    if changed
        .iter()
        .any(|path| broadening_for_path(path) == Some(SelectionBroadening::RootConfig))
    {
        warnings.push("root_config_changed:all_changed".to_owned());
        return all_packages;
    }
    if let Some(warning) = changed
        .iter()
        .find_map(|path| selection_broadens_for_path(path))
    {
        warnings.push(warning.to_owned());
        return all_packages;
    }
    let Some((rust_changed, tofu_affected)) =
        crate::select_tofu::split_or_broaden(root, base, head, discovery, changed, warnings)
    else {
        return all_packages;
    };
    let Some(resolved) = resolved_edges(discovery, warnings) else {
        return all_packages;
    };
    let ownership = Ownership::new(discovery);
    let Some(links) = resolved_links(root, &ownership, warnings) else {
        return all_packages;
    };
    let ownership = ownership.with_links(links);
    if has_unowned_file(&ownership, &rust_changed) {
        warnings.push("unclassified_files:all_changed".to_owned());
        return all_packages;
    }
    let head_edges = head_edges(discovery);
    let mut affected = match base_edges(root, base, head, discovery) {
        Ok(base_edges) => affected_packages(
            &ownership,
            &rust_changed,
            &[&base_edges, &head_edges, &resolved],
        ),
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return all_packages;
        }
    };
    affected.extend(tofu_affected);
    affected
}

/// True when one task counts as changed under the affected packages.
///
/// Tasks with an empty unit ID follow their manifest siblings: a
/// workspace-level task is affected when any same-manifest package is.
pub(crate) fn group_changed(
    task: &ProposedTask,
    changed: &BTreeSet<String>,
    changed_keys: &BTreeSet<String>,
) -> bool {
    changed.contains(&task.identity.unit_id)
        || (task.identity.unit_id.is_empty() && changed_keys.contains(&task.identity.unit_key))
}

/// Files changed between base and head via the allowlisted `diff` verb.
///
/// Validation gates the untrusted range (flag-injection defense); `-z` is
/// our own trusted constant added after, so output is NUL-delimited with
/// no C-quoting or trimming. Rename detection stays off: the output is a
/// set of possibly-affected paths, and a collapsed R100 rename would drop
/// the old path's owner from selection. Same convention as
/// `tree_diff_names` and `added_files`.
fn changed_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    validate_diff_rev(head, "bad_head")?;
    let mut args = vec![
        OsString::from("--name-only"),
        OsString::from("--no-renames"),
        OsString::from(format!("{base}...{head}")),
        OsString::from("--"),
    ];
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    args.insert(0, OsString::from("-z"));
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    split_nul_paths(&output.stdout)
}

/// Committed change set minus advisory tool files, or `None` to broaden.
///
/// Git failures, non-ignored untracked files, and modified tracked files
/// broaden with a recorded warning; the flag reports whether tool files were filtered, for the
/// findings-only warning when nothing else changed.
fn change_sets(
    root: &Path,
    base: &str,
    head: &str,
    warnings: &mut Vec<String>,
) -> Option<(BTreeSet<String>, bool)> {
    let changed = match changed_files(root, base, head) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    let untracked = match untracked_files(root) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    let mut dirty = BTreeSet::new();
    for cached in [true, false] {
        match tree_diff_names(root, cached) {
            Ok(files) => dirty.extend(files),
            Err(problem) => {
                warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
                return None;
            }
        }
    }
    if changed.iter().any(|path| is_project_tool_source(path)) {
        warnings.push(PROJECT_TOOL_CONFIG_CHANGED.to_owned());
        return None;
    }
    let toolfiles = changed.iter().any(|path| is_advisory_toolfile(path))
        || untracked.iter().any(|path| is_advisory_toolfile(path));
    if untracked
        .into_iter()
        .any(|path| !is_advisory_toolfile(&path))
    {
        warnings.push("untracked_files:all_changed".to_owned());
        return None;
    }
    // The tree under test must be the committed head: a modified tracked
    // file is a change the commit diff cannot see.
    if dirty.iter().any(|path| !is_advisory_toolfile(path)) {
        warnings.push("dirty_worktree:all_changed".to_owned());
        return None;
    }
    let changed = changed
        .into_iter()
        .filter(|path| !is_advisory_toolfile(path))
        .collect();
    Some((changed, toolfiles))
}

/// Working-tree change set minus advisory tool files, or `None` to broaden.
///
/// Staged plus unstaged diffs plus untracked files form the local union;
/// git failures broaden with a recorded warning.
fn local_change_set(root: &Path, warnings: &mut Vec<String>) -> Option<(BTreeSet<String>, bool)> {
    let mut changed = BTreeSet::new();
    for cached in [true, false] {
        match tree_diff_names(root, cached) {
            Ok(files) => changed.extend(files),
            Err(problem) => {
                warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
                return None;
            }
        }
    }
    let untracked = match untracked_files(root) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    changed.extend(untracked);
    if changed.iter().any(|path| is_project_tool_source(path)) {
        warnings.push(PROJECT_TOOL_CONFIG_CHANGED.to_owned());
        return None;
    }
    let toolfiles = changed.iter().any(|path| is_advisory_toolfile(path));
    let changed = changed
        .into_iter()
        .filter(|path| !is_advisory_toolfile(path))
        .collect();
    Some((changed, toolfiles))
}

#[cfg(test)]
#[path = "group_selection_tests.rs"]
mod group_selection_tests;

#[cfg(test)]
#[path = "select_matrix_tests.rs"]
mod matrix_tests;
#[cfg(test)]
#[path = "select_pr_range_tests.rs"]
mod select_pr_range_tests;
