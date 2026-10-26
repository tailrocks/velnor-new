// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{OrchestratorError, internal::internal};

pub(crate) struct Entry<'a> {
    pub(crate) path: &'a str,
    pub(crate) mode: &'a str,
    pub(crate) kind: &'a str,
    pub(crate) sha: &'a str,
}

pub(crate) fn validate(entries: &[Entry<'_>], limit: usize) -> Result<(), OrchestratorError> {
    validate_inner(entries, limit, None)
}

pub(crate) fn validate_protected_symlink_base(
    entries: &[Entry<'_>],
    limit: usize,
    path: &str,
    blob_sha: &str,
) -> Result<(), OrchestratorError> {
    validate_inner(entries, limit, Some((path, blob_sha)))
}

fn validate_inner(
    entries: &[Entry<'_>],
    limit: usize,
    exact_symlink: Option<(&str, &str)>,
) -> Result<(), OrchestratorError> {
    if entries.is_empty() || entries.len() > limit {
        return Err(internal("tap_tree_inventory_invalid"));
    }
    let mut paths = BTreeMap::new();
    for entry in entries {
        if paths.insert(entry.path, entry).is_some() {
            return Err(internal("tap_tree_duplicate_path"));
        }
        if entry.mode == "040000" && entry.kind == "tree" {
            velnor_actions_mise::validate_tap_regular_leaf(
                entry.path, 0o100_644, "blob", entry.sha,
            )
            .map_err(|_| internal("tap_tree_directory_invalid"))?;
        } else if entry.mode == "120000"
            && entry.kind == "blob"
            && exact_symlink.is_some_and(|(path, sha)| entry.path == path && entry.sha == sha)
        {
            velnor_actions_mise::validate_tap_regular_leaf(
                entry.path, 0o100_644, "blob", entry.sha,
            )
            .map_err(|_| internal("tap_tree_symlink_invalid"))?;
        } else {
            let mode = match entry.mode {
                "100644" => 0o100_644,
                "100755" => 0o100_755,
                _ => return Err(internal("tap_tree_nonregular_entry")),
            };
            velnor_actions_mise::validate_tap_regular_leaf(entry.path, mode, entry.kind, entry.sha)
                .map_err(|_| internal("tap_tree_nonregular_entry"))?;
        }
    }
    for entry in entries {
        for (index, _) in entry.path.match_indices('/') {
            let parent = &entry.path[..index];
            if !paths
                .get(parent)
                .is_some_and(|item| item.mode == "040000" && item.kind == "tree")
            {
                return Err(internal("tap_tree_parent_missing"));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_formula_delta(
    base: &[Entry<'_>],
    head: &[Entry<'_>],
    formula: &str,
    limit: usize,
) -> Result<(), OrchestratorError> {
    validate(base, limit)?;
    validate(head, limit)?;
    if base.len() != head.len() {
        return Err(internal("tap_tree_delta_inventory_changed"));
    }
    let before = base
        .iter()
        .map(|entry| (entry.path, entry))
        .collect::<BTreeMap<_, _>>();
    let after = head
        .iter()
        .map(|entry| (entry.path, entry))
        .collect::<BTreeMap<_, _>>();
    if !before.contains_key(formula) {
        return Err(internal("tap_tree_delta_formula_missing"));
    }
    for (path, old) in &before {
        let new = after
            .get(path)
            .ok_or_else(|| internal("tap_tree_delta_path_removed"))?;
        if *path == formula {
            if old.mode != "100644"
                || new.mode != "100644"
                || old.kind != "blob"
                || new.kind != "blob"
            {
                return Err(internal("tap_tree_delta_formula_mode_invalid"));
            }
        } else if is_formula_ancestor(path, formula) {
            if old.mode != "040000"
                || new.mode != "040000"
                || old.kind != "tree"
                || new.kind != "tree"
            {
                return Err(internal("tap_tree_delta_ancestor_invalid"));
            }
        } else if old.mode != new.mode || old.kind != new.kind || old.sha != new.sha {
            return Err(internal("tap_tree_delta_unexpected_change"));
        }
    }
    Ok(())
}

fn is_formula_ancestor(path: &str, formula: &str) -> bool {
    formula
        .strip_prefix(path)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
