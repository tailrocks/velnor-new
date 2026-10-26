// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

use super::paths::{is_advisory_toolfile, tree_diff_names, untracked_files};
use super::tool_sources;
use crate::git_paths::split_nul_paths;
use crate::validators::{validate_diff_rev, validate_select_diff_args};

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
    let unchanged =
        tool_sources::unchanged_declaration_sources(root, base, Some(head), &changed, warnings)?;
    let toolfiles = !unchanged.is_empty()
        || changed.iter().any(|path| is_advisory_toolfile(path))
        || untracked.iter().any(|path| is_advisory_toolfile(path));
    if untracked
        .into_iter()
        .any(|path| !is_advisory_toolfile(&path))
    {
        warnings.push("untracked_files:all_changed".to_owned());
        return None;
    }
    if dirty.iter().any(|path| !is_advisory_toolfile(path)) {
        warnings.push("dirty_worktree:all_changed".to_owned());
        return None;
    }
    let changed = changed
        .into_iter()
        .filter(|path| !is_advisory_toolfile(path) && !unchanged.contains(path))
        .collect();
    Some((changed, toolfiles))
}

pub(super) fn local_change_set(
    root: &Path,
    base: &str,
    warnings: &mut Vec<String>,
) -> Option<(BTreeSet<String>, bool)> {
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
    let unchanged =
        tool_sources::unchanged_declaration_sources(root, base, None, &changed, warnings)?;
    let toolfiles = !unchanged.is_empty() || changed.iter().any(|path| is_advisory_toolfile(path));
    let changed = changed
        .into_iter()
        .filter(|path| !is_advisory_toolfile(path) && !unchanged.contains(path))
        .collect();
    Some((changed, toolfiles))
}
