//! Obligation universe and changed-work classification for `plan-v1`.
//! The plan carries the full configured obligation universe; changed-work
//! analysis classifies obligations conservatively and qualifies whether
//! complete semantic proof may refine directory ownership hints. Nothing is
//! ever removed for being unaffected: without proof, everything executes.

#[path = "select_checkout.rs"]
mod checkout;

use checkout::head_sha;
pub(crate) use checkout::verify_checkout;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{ProposedTask, Stack, WorkflowEvent};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::SelectionBroadening;

use crate::decisions::{broadening_for_path, selection_broadens_for_path};
use crate::discover::Discovery;
use crate::git_paths::{NON_UTF8_PATH, split_nul_paths};
use crate::select_affected::{affected_packages, has_unowned_file, workspace_config_changed};
use crate::select_edges::{base_graph, candidate_graph};
use crate::validators::{validate_diff_rev, validate_select_diff_args};

#[cfg(test)]
#[path = "select_evidence_tests.rs"]
mod evidence_tests;

/// Conservative directory hints plus qualified semantic-proof eligibility.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChangedSelection {
    /// Units that execute unless a permitted complete proof covers them.
    pub(crate) affected: BTreeSet<String>,
    /// Rust units whose hints an exact complete consumed-input proof may refine.
    pub(crate) proof_refinable: BTreeSet<String>,
}

impl ChangedSelection {
    /// A classification whose changed hints cannot be refined by coverage.
    fn required(affected: BTreeSet<String>) -> Self {
        Self {
            affected,
            proof_refinable: BTreeSet::new(),
        }
    }

    /// Qualified ownership hints require live semantic proof before refinement.
    fn qualified(affected: BTreeSet<String>) -> Self {
        Self {
            proof_refinable: affected.clone(),
            affected,
        }
    }

    /// Refinement is qualified only for the Rust semantic input resolver.
    fn restrict_to_rust(mut self, tasks: &[ProposedTask]) -> Self {
        for task in tasks {
            if Stack::from_id(&task.stack_id) != Some(Stack::Rust) {
                self.proof_refinable.remove(&task.identity.unit_id);
            }
        }
        self
    }
}

/// Full obligation universe: every task with applicable targets.
///
/// Targetless tasks are not obligations. Each omission is recorded as a
/// `valid_no_test_targets:<task-id>` warning, never silent. Tofu
/// subdir roots additionally record one `path.cwd:<root>` caveat each.
pub(crate) fn select_universe<'a>(
    discovery: &'a Discovery,
    warnings: &mut Vec<String>,
) -> Vec<&'a ProposedTask> {
    let mut kept = Vec::new();
    for task in &discovery.proposals {
        if task.no_targets {
            warnings.push(format!("valid_no_test_targets:{}", task.task_id));
        } else {
            kept.push(task);
        }
    }
    crate::select_tofu::push_chdir_findings(discovery, warnings);
    kept
}

/// Changed package hints and qualified uncertainty, or an unknown comparison.
///
/// `None` marks every obligation changed (fail-open broad: the correct
/// unknown-comparison behavior). An empty set means nothing changed, so
/// obligations stay eligible for verified coverage instead of executing
/// unconditionally. Committed events compare exact base and candidate trees.
pub(crate) fn classify_changed(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &Discovery,
    warnings: &mut Vec<String>,
) -> Option<ChangedSelection> {
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
        if matches!(
            event,
            WorkflowEvent::PullRequest
                | WorkflowEvent::MergeGroup
                | WorkflowEvent::Fork
                | WorkflowEvent::Push
                | WorkflowEvent::WorkflowDispatch
        ) {
            warnings.push("comparison_unavailable:missing_base:all_changed".to_owned());
        }
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
) -> Option<ChangedSelection> {
    let sha = match head_sha(root) {
        Ok(sha) => sha,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    let (changed, toolfiles) = local_change_set(root, warnings)?;
    let mut selection =
        affected_from_changed(discovery, &changed, toolfiles, root, &sha, &sha, warnings);
    selection.proof_refinable.clear();
    Some(selection)
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
) -> ChangedSelection {
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
        return ChangedSelection::required(BTreeSet::new());
    }
    if changed
        .iter()
        .any(|path| broadening_for_path(path) == Some(SelectionBroadening::Lockfile))
    {
        warnings.push("cargo_lock_changed:all_changed".to_owned());
        return ChangedSelection::required(all_packages);
    }
    if changed
        .iter()
        .any(|path| broadening_for_path(path) == Some(SelectionBroadening::RootConfig))
        || workspace_config_changed(discovery, changed)
    {
        warnings.push("root_config_changed:all_changed".to_owned());
        return ChangedSelection::required(all_packages);
    }
    if let Some(warning) = changed
        .iter()
        .find_map(|path| selection_broadens_for_path(path))
    {
        warnings.push(warning.to_owned());
        return ChangedSelection::required(all_packages);
    }
    let Some((rust_changed, tofu_affected)) =
        crate::select_tofu::split_or_broaden(root, base, head, discovery, changed, warnings)
    else {
        return ChangedSelection::required(all_packages);
    };
    let graphs = base_graph(root, base, discovery)
        .and_then(|base| candidate_graph(root, discovery).map(|candidate| (base, candidate)));
    let (base_graph, candidate_graph) = match graphs {
        Ok(graphs) => graphs,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return ChangedSelection::required(all_packages);
        }
    };
    let mut affected = affected_packages(
        discovery,
        &rust_changed,
        &base_graph.edges,
        &candidate_graph.edges,
        &base_graph.owners,
    );
    affected.extend(tofu_affected);
    if has_unowned_file(discovery, &rust_changed, &base_graph.owners) {
        warnings.push("unclassified_files:all_changed".to_owned());
        return ChangedSelection::qualified(all_packages).restrict_to_rust(tasks);
    }
    ChangedSelection::qualified(affected).restrict_to_rust(tasks)
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
        OsString::from(base),
        OsString::from(head),
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
/// Git failures and non-ignored untracked files broaden with a recorded
/// warning; the flag reports whether tool files were filtered, for the
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
    let toolfiles = changed.iter().any(|path| is_advisory_toolfile(path))
        || untracked.iter().any(|path| is_advisory_toolfile(path));
    if untracked
        .into_iter()
        .any(|path| !is_advisory_toolfile(&path))
    {
        warnings.push("untracked_files:all_changed".to_owned());
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
    let toolfiles = changed.iter().any(|path| is_advisory_toolfile(path));
    let changed = changed
        .into_iter()
        .filter(|path| !is_advisory_toolfile(path))
        .collect();
    Some((changed, toolfiles))
}

/// Staged (`--cached`) or unstaged names, NUL-delimited like `changed_files`.
fn tree_diff_names(root: &Path, cached: bool) -> Result<BTreeSet<String>, String> {
    let mut args = vec![OsString::from("--name-only")];
    if cached {
        args.push(OsString::from("--cached"));
    }
    args.push(OsString::from("--no-renames"));
    args.push(OsString::from("--"));
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

/// Untracked non-ignored paths via the allowlisted `ls-files` verb.
///
/// Ignored paths never surface (`--exclude-standard`); any other untracked
/// file broadens via the caller because the committed diff cannot see it.
fn untracked_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let args = ["--others", "--exclude-standard", "-z"]
        .iter()
        .map(OsString::from)
        .collect();
    let output = GitRequest::ls_files(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    split_nul_paths(&output.stdout)
}

/// True for advisory tool files: findings-only, never select or broaden.
///
/// Generated execution uses Velnor's exact pins, so these repository inputs
/// feed inspection findings only; a task consuming one must declare it.
fn is_advisory_toolfile(path: &str) -> bool {
    path == ".mise.toml" || velnor_actions_rust::is_known_toolfile(path)
}
