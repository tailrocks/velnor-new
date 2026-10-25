use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{ProposedTask, WorkflowEvent};
use velnor_actions_rust::SelectionBroadening;

use crate::decisions::{broadening_for_path, selection_broadens_for_path};
use crate::discover::Discovery;
use crate::git_paths::NON_UTF8_PATH;
use crate::select_affected::{Ownership, affected_packages, has_unowned_file};
use crate::select_edges::{base_edges, head_edges};

#[path = "select_ansible.rs"]
pub(crate) mod ansible;
#[path = "select_bun.rs"]
pub(crate) mod bun;
#[path = "select_changes.rs"]
mod changes;
#[path = "select_checkout.rs"]
mod checkout;
#[path = "select_dbt.rs"]
pub(crate) mod dbt;
#[path = "select_images.rs"]
pub(crate) mod images;
pub(crate) use images::ChangeEvidence;
#[path = "select_mode.rs"]
mod mode;
#[path = "select_paths.rs"]
mod paths;
use checkout::checkout_bound_to_base;
pub(crate) use checkout::{verify_checkout, verify_checkout_until};
pub(crate) use mode::{SelectionMode, selection_mode};
use mode::{event_token, resolved_edges, resolved_links};

#[path = "select_tool_sources.rs"]
mod tool_sources;

use changes::{change_sets, local_change_set};
use paths::head_sha;

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

pub(crate) fn classify_changed(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &Discovery,
    warnings: &mut Vec<String>,
    evidence: &mut ChangeEvidence,
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
        return classify_local(root, discovery, warnings, evidence);
    }
    let Some(base) = base else {
        warnings.push("comparison_unavailable:missing_base:all_changed".to_owned());
        return None;
    };
    match checkout_bound_to_base(root, base, head) {
        Ok(true) => {}
        Ok(false) => {
            warnings.push("comparison_unavailable:stale_merge_ref:all_changed".to_owned());
            return None;
        }
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    }
    let (changed, toolfiles) = change_sets(root, base, head, warnings)?;
    Some(affected_from_changed(
        discovery,
        &changed,
        toolfiles,
        root,
        (base, head),
        warnings,
        evidence,
    ))
}

fn classify_local(
    root: &Path,
    discovery: &Discovery,
    warnings: &mut Vec<String>,
    evidence: &mut ChangeEvidence,
) -> Option<BTreeSet<String>> {
    let sha = match head_sha(root) {
        Ok(sha) => sha,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    let (changed, toolfiles) = local_change_set(root, &sha, warnings)?;
    Some(affected_from_changed(
        discovery,
        &changed,
        toolfiles,
        root,
        (&sha, &sha),
        warnings,
        evidence,
    ))
}

fn affected_from_changed(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    toolfiles: bool,
    root: &Path,
    (base, head): (&str, &str),
    warnings: &mut Vec<String>,
    evidence: &mut ChangeEvidence,
) -> BTreeSet<String> {
    if !changed
        .iter()
        .any(|path| selection_broadens_for_path(path).is_some())
    {
        evidence.paths = Some(changed.clone());
    }
    let all_packages: BTreeSet<String> = discovery
        .proposals
        .iter()
        .map(|task| task.identity.unit_id.clone())
        .collect();
    let tofu_affected = if changed.is_empty() {
        warnings.push(
            if toolfiles {
                "toolfiles_only:all_unchanged"
            } else {
                "no_affected_files:all_unchanged"
            }
            .to_owned(),
        );
        BTreeSet::new()
    } else {
        if let Some(warning) = broadening_warning(changed) {
            warnings.push(warning);
            return all_packages;
        }
        let Some(keys) = crate::select_tofu::tofu_keys_or_broaden(
            root, base, head, discovery, changed, warnings,
        ) else {
            return all_packages;
        };
        keys
    };
    let Some(mut affected) = rust_affected(discovery, changed, root, base, head, warnings) else {
        return all_packages;
    };
    affected.extend(tofu_affected);
    affected
}

fn broadening_warning(changed: &BTreeSet<String>) -> Option<String> {
    for (kind, warning) in [
        (
            SelectionBroadening::Lockfile,
            "cargo_lock_changed:all_changed",
        ),
        (
            SelectionBroadening::RootConfig,
            "root_config_changed:all_changed",
        ),
    ] {
        if changed
            .iter()
            .any(|path| broadening_for_path(path) == Some(kind))
        {
            return Some(warning.to_owned());
        }
    }
    changed
        .iter()
        .find_map(|path| selection_broadens_for_path(path))
        .map(str::to_owned)
}

fn rust_affected(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    root: &Path,
    base: &str,
    head: &str,
    warnings: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    let resolved = resolved_edges(discovery, warnings)?;
    let ownership = Ownership::new(discovery);
    let links = resolved_links(root, &ownership, warnings)?;
    let ownership = ownership.with_links(links);
    if has_unowned_file(&ownership, changed) {
        warnings.push("unclassified_files:all_changed".to_owned());
        return None;
    }
    let forced = match crate::select_reads::forced_packages(root, discovery, &ownership) {
        Ok(forced) => forced,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            return None;
        }
    };
    warnings.extend(crate::select_reads::forced_warnings(&ownership, &forced));
    let forced = crate::select_reads::forced_ids(&forced);
    if changed.is_empty() && forced.is_empty() {
        return Some(BTreeSet::new());
    }
    let head_edges = head_edges(discovery);
    match base_edges(root, base, head, discovery) {
        Ok(base_edges) => Some(affected_packages(
            &ownership,
            changed,
            &forced,
            &[&base_edges, &head_edges, &resolved],
        )),
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            None
        }
    }
}

pub(crate) fn group_changed(
    task: &ProposedTask,
    changed: &BTreeSet<String>,
    changed_keys: &BTreeSet<String>,
) -> bool {
    changed.contains(&task.identity.unit_id)
        || (task.identity.unit_id.is_empty() && changed_keys.contains(&task.identity.unit_key))
}
