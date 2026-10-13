//! Event-time affected-work selection for `plan-v1`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{FOREIGN_TOOL_FILES, RUST_TOOLCHAIN_FILE, TaskGroup};

use crate::decisions::selection_broadens_for_path;
use crate::discover::Discovery;
use crate::select_affected::{affected_packages, has_unowned_file};
use crate::select_edges::{base_edges, head_edges};
use crate::validators::{validate_diff_rev, validate_select_diff_args};

/// Select task groups: affected subset on PRs with a base, else all.
///
/// Groups without applicable targets are never selected: scheduling them
/// would emit impossible obligations (for example `cargo test --doc` for a
/// package with no doctest-able target). Each omission is recorded as a
/// `valid_no_test_targets:<task-id>` warning, never silent.
pub(crate) fn select_groups<'a>(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &'a Discovery,
    warnings: &mut Vec<String>,
) -> Vec<&'a TaskGroup> {
    let mut kept = Vec::new();
    for group in select_candidate_groups(root, event, base, head, discovery, warnings) {
        if group.no_test_targets {
            warnings.push(format!("valid_no_test_targets:{}", group.task_id));
        } else {
            kept.push(group);
        }
    }
    kept
}

/// Candidate task groups before the valid-no-target exclusion.
fn select_candidate_groups<'a>(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &'a Discovery,
    warnings: &mut Vec<String>,
) -> Vec<&'a TaskGroup> {
    let all: Vec<&TaskGroup> = discovery.task_groups.iter().collect();
    if event == WorkflowEvent::Local {
        return select_local(discovery, root, warnings);
    }
    let narrows = matches!(
        event,
        WorkflowEvent::PullRequest | WorkflowEvent::MergeGroup | WorkflowEvent::Fork
    );
    if !(narrows && base.is_some()) {
        if narrows {
            warnings.push("comparison_unavailable:missing_base:selecting_all".to_owned());
        }
        return all;
    }
    let base = base.unwrap_or_default();
    let Some((changed, toolfiles)) = change_sets(root, base, head, warnings) else {
        return all;
    };
    narrow_from_changed(discovery, &changed, toolfiles, root, base, head, warnings)
}

/// Local pre-push selection: the working tree against `HEAD`.
fn select_local<'a>(
    discovery: &'a Discovery,
    root: &Path,
    warnings: &mut Vec<String>,
) -> Vec<&'a TaskGroup> {
    let all: Vec<&'a TaskGroup> = discovery.task_groups.iter().collect();
    let sha = match head_sha(root) {
        Ok(sha) => sha,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return all;
        }
    };
    let Some((changed, toolfiles)) = local_change_set(root, warnings) else {
        return all;
    };
    narrow_from_changed(discovery, &changed, toolfiles, root, &sha, &sha, warnings)
}

/// Narrow one change set to owning groups, broadening on risk.
fn narrow_from_changed<'a>(
    discovery: &'a Discovery,
    changed: &BTreeSet<String>,
    toolfiles: bool,
    root: &Path,
    base: &str,
    head: &str,
    warnings: &mut Vec<String>,
) -> Vec<&'a TaskGroup> {
    let all: Vec<&TaskGroup> = discovery.task_groups.iter().collect();
    if changed.is_empty() {
        warnings.push(
            if toolfiles {
                "toolfiles_only:findings_only"
            } else {
                "no_affected_files"
            }
            .to_owned(),
        );
        return Vec::new();
    }
    if changed.iter().any(|path| path == "Cargo.lock") {
        warnings.push("cargo_lock_changed:selecting_all".to_owned());
        return all;
    }
    if changed.iter().any(|path| is_root_config(path)) {
        warnings.push("root_config_changed:selecting_all".to_owned());
        return all;
    }
    if let Some(warning) = changed
        .iter()
        .find_map(|path| selection_broadens_for_path(path))
    {
        warnings.push(warning.to_owned());
        return all;
    }
    if has_unowned_file(discovery, changed) {
        warnings.push("unclassified_files:selecting_all".to_owned());
        return all;
    }
    let head_edges = head_edges(discovery);
    let base_edges = match base_edges(root, base, head, discovery) {
        Ok(edges) => edges,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return all;
        }
    };
    let selected_ids = affected_packages(discovery, changed, &base_edges, &head_edges);
    let mut keys = BTreeSet::new();
    for group in &all {
        if selected_ids.contains(&group.package_id) {
            keys.insert(group.manifest_key.clone());
        }
    }
    all.into_iter()
        .filter(|group| {
            selected_ids.contains(&group.package_id)
                || (group.package_id.is_empty() && keys.contains(&group.manifest_key))
        })
        .collect()
}

/// Files changed between base and head via the allowlisted `diff` verb.
///
/// The trailing `--` separates the revision range from paths; a `--` before
/// the range would misparse the range as a path, so validation above is the
/// flag-injection defense and the separator is belt and braces.
fn changed_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    validate_diff_rev(head, "bad_head")?;
    let range = format!("{base}...{head}");
    let args = vec![
        OsString::from("--name-only"),
        OsString::from(range),
        OsString::from("--"),
    ];
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
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
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return None;
        }
    };
    let untracked = match untracked_files(root) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return None;
        }
    };
    let toolfiles = changed.iter().any(|path| is_advisory_toolfile(path))
        || untracked.iter().any(|path| is_advisory_toolfile(path));
    if untracked
        .into_iter()
        .any(|path| !is_advisory_toolfile(&path))
    {
        warnings.push("untracked_files:selecting_all".to_owned());
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
                warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
                return None;
            }
        }
    }
    let untracked = match untracked_files(root) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
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

/// Names from the staged (`--cached`) or unstaged working-tree diff.
fn tree_diff_names(root: &Path, cached: bool) -> Result<BTreeSet<String>, String> {
    let mut args = vec![OsString::from("--name-only")];
    if cached {
        args.push(OsString::from("--cached"));
    }
    args.push(OsString::from("--no-renames"));
    args.push(OsString::from("--"));
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Resolve `HEAD` to a SHA for local comparison.
fn head_sha(root: &Path) -> Result<String, String> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    if !output.success {
        return Err("missing_head".to_owned());
    }
    let sha = output
        .stdout_text("git")
        .map_err(|err| err.to_string())?
        .trim()
        .to_owned();
    validate_diff_rev(&sha, "bad_head")?;
    Ok(sha)
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
    let mut out = BTreeSet::new();
    for chunk in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let path = String::from_utf8(chunk.to_vec()).map_err(|err| err.to_string())?;
        out.insert(path);
    }
    Ok(out)
}

/// True for advisory tool files: findings-only, never select or broaden.
///
/// Generated execution uses Velnor's exact pins, so these repository inputs
/// feed inspection findings only; a task consuming one must declare it.
fn is_advisory_toolfile(path: &str) -> bool {
    path == RUST_TOOLCHAIN_FILE || path == ".mise.toml" || FOREIGN_TOOL_FILES.contains(&path)
}

/// True for root Cargo config paths: root manifest or cargo config.
fn is_root_config(path: &str) -> bool {
    path == "Cargo.toml" || path == ".cargo/config.toml" || path == ".cargo/config"
}
