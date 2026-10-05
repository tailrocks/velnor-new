//! Git-backed change-set acquisition for obligation classification.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::validators::{validate_diff_rev, validate_select_diff_args};

use super::affected_from_changed;

/// Local pre-push classification: the working tree against `HEAD`.
pub(super) fn classify_local(
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
/// Second parent of the checkout merge commit, if any.
pub(super) fn second_parent(root: &Path) -> Option<String> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD^2")])
        .run_in(root)
        .ok()?;
    if !output.success {
        return None;
    }
    let sha = output.stdout_text("git").ok()?.trim().to_owned();
    validate_diff_rev(&sha, "bad_head").ok()?;
    Some(sha)
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
/// Git failures and non-ignored untracked files broaden with a recorded
/// warning; the flag reports whether tool files were filtered, for the
/// findings-only warning when nothing else changed.
pub(super) fn change_sets(
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

/// Resolve `HEAD` to a SHA for local comparison.
pub(super) fn head_sha(root: &Path) -> Result<String, String> {
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
    split_nul_paths(&output.stdout)
}

/// True for advisory tool files: findings-only, never select or broaden.
///
/// Generated execution uses Velnor's exact pins, so these repository inputs
/// feed inspection findings only; a task consuming one must declare it.
fn is_advisory_toolfile(path: &str) -> bool {
    path == ".mise.toml" || velnor_actions_rust::is_known_toolfile(path)
}
