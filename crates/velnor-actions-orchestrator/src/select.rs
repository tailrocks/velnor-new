//! Obligation universe and changed-work classification for `plan-v1`.
//!
//! The plan carries the full configured obligation universe; changed-work
//! analysis only classifies obligations as changed (must execute) or
//! unchanged (eligible for verified reuse/baseline coverage). Nothing is
//! ever removed for being unaffected: without proof, everything executes.

#[path = "select_changes.rs"]
mod changes;

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{ProposedTask, WorkflowEvent};
use velnor_actions_rust::SelectionBroadening;

use crate::OrchestratorError;
use crate::decisions::{broadening_for_path, selection_broadens_for_path};
use crate::discover::Discovery;
use crate::git_paths::NON_UTF8_PATH;
use crate::internal::internal;
use crate::select_affected::{affected_packages, has_unowned_file};
use crate::select_edges::{base_edges, head_edges};
use crate::validators::validate_diff_rev;

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
    if event == WorkflowEvent::Qualification {
        return Some(
            discovery
                .proposals
                .iter()
                .map(|task| task.identity.unit_id.clone())
                .collect(),
        );
    }
    if discovery.skipped_non_utf8 {
        warnings.push(format!(
            "comparison_unavailable:{NON_UTF8_PATH}:all_changed"
        ));
        return None;
    }
    if event == WorkflowEvent::Local {
        return changes::classify_local(root, discovery, warnings);
    }
    let Some(base) = base else {
        if matches!(
            event,
            WorkflowEvent::PullRequest
                | WorkflowEvent::MergeGroup
                | WorkflowEvent::Fork
                | WorkflowEvent::Push
        ) {
            warnings.push("comparison_unavailable:missing_base:all_changed".to_owned());
        }
        return None;
    };
    let (changed, toolfiles) = changes::change_sets(root, base, head, warnings)?;
    Some(affected_from_changed(
        discovery, &changed, toolfiles, root, base, head, warnings,
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
    if has_unowned_file(discovery, &rust_changed) {
        warnings.push("unclassified_files:all_changed".to_owned());
        return all_packages;
    }
    let head_edges = head_edges(discovery);
    let mut affected = match base_edges(root, base, head, discovery) {
        Ok(base_edges) => affected_packages(discovery, &rust_changed, &base_edges, &head_edges),
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

/// Verify the analyzed checkout matches the intended head.
///
/// Identities describe the working tree; a checkout at any other commit
/// would validate the wrong tree. Push and merge-group runs resolve
/// `HEAD` exactly; PR and fork runs additionally accept the merge
/// checkout (`HEAD^2`), which is what would land. Local runs analyze
/// the working tree itself and skip this check.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for checkout/head mismatch or
/// unresolvable `HEAD`.
pub(crate) fn verify_checkout(
    root: &Path,
    event: WorkflowEvent,
    head: &str,
) -> Result<(), OrchestratorError> {
    if event == WorkflowEvent::Local {
        return Ok(());
    }
    validate_diff_rev(head, "bad_head").map_err(|problem| internal(&problem))?;
    let checkout =
        changes::head_sha(root).map_err(|problem| internal(&format!("bad_checkout:{problem}")))?;
    if checkout == head {
        return Ok(());
    }
    if matches!(event, WorkflowEvent::PullRequest | WorkflowEvent::Fork)
        && changes::second_parent(root).as_deref() == Some(head)
    {
        return Ok(());
    }
    Err(internal("checkout_head_mismatch"))
}
