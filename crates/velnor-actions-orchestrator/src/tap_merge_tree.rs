//! Full Git tree comparison for the writer's single-formula commit.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

use super::{
    candidate::{MergeCandidate, validate_formula_at_base},
    github::TapMergeGitHub,
};

const TREE_LIMIT: usize = 100_000;

pub(super) fn verify_formula_delta_at_base(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
    base_sha: &str,
    head_sha: &str,
    expected_formula: &[u8],
) -> Result<(), OrchestratorError> {
    let base = commit_tree(api, &candidate.policy.repository, base_sha)?;
    let head = commit_tree(api, &candidate.policy.repository, head_sha)?;
    let base_entries = recursive_tree(api, &candidate.policy.repository, &base)?;
    let head_entries = recursive_tree(api, &candidate.policy.repository, &head)?;
    if base_entries.len() != head_entries.len() {
        return Err(internal("tap_merge_tree_inventory_changed"));
    }
    let formula = candidate.formula_path.as_str();
    for (path, before) in &base_entries {
        let after = head_entries
            .get(path)
            .ok_or_else(|| internal("tap_merge_tree_path_removed"))?;
        if path == formula {
            if before.0 != "100644"
                || after.0 != "100644"
                || before.1 != "blob"
                || after.1 != "blob"
            {
                return Err(internal("tap_merge_formula_mode_invalid"));
            }
        } else if before != after {
            return Err(internal("tap_merge_unexpected_tree_change"));
        }
    }
    if !base_entries.contains_key(formula) {
        return Err(internal("tap_merge_formula_missing_at_base"));
    }
    let base_formula =
        super::observation::contents(api, &candidate.policy.repository, formula, base_sha)?;
    let head_formula =
        super::observation::contents(api, &candidate.policy.repository, formula, head_sha)?;
    if head_formula != expected_formula {
        return Err(internal("tap_merge_formula_head_mismatch"));
    }
    validate_formula_at_base(candidate, base_sha, &base_formula, expected_formula)
}

fn commit_tree(
    api: &TapMergeGitHub,
    repository: &str,
    sha: &str,
) -> Result<String, OrchestratorError> {
    let commit: Value = api.get(&format!("repos/{repository}/git/commits/{sha}"))?;
    commit
        .pointer("/tree/sha")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 40 && is_lower_hex(value))
        .map(str::to_owned)
        .ok_or_else(|| internal("tap_merge_tree_sha_invalid"))
}

fn recursive_tree(
    api: &TapMergeGitHub,
    repository: &str,
    sha: &str,
) -> Result<BTreeMap<String, (String, String, String)>, OrchestratorError> {
    let response: Value = api.get(&format!("repos/{repository}/git/trees/{sha}?recursive=1"))?;
    let entries = response["tree"]
        .as_array()
        .filter(|entries| entries.len() <= TREE_LIMIT)
        .ok_or_else(|| internal("tap_merge_tree_unreadable"))?;
    if response["sha"].as_str() != Some(sha) || response["truncated"].as_bool() != Some(false) {
        return Err(internal("tap_merge_tree_truncated"));
    }
    let mut result = BTreeMap::new();
    for entry in entries {
        let path = entry["path"]
            .as_str()
            .filter(|path| {
                !path.is_empty()
                    && !path.starts_with('/')
                    && !path.split('/').any(|part| matches!(part, "." | ".."))
            })
            .ok_or_else(|| internal("tap_merge_tree_path_invalid"))?;
        let mode = entry["mode"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_tree_mode_missing"))?;
        let object_type = entry["type"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_tree_type_missing"))?;
        let object_sha = entry["sha"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_tree_object_missing"))?;
        if !is_lower_hex(object_sha)
            || result
                .insert(
                    path.to_owned(),
                    (
                        mode.to_owned(),
                        object_type.to_owned(),
                        object_sha.to_owned(),
                    ),
                )
                .is_some()
        {
            return Err(internal("tap_merge_tree_duplicate_or_invalid"));
        }
    }
    Ok(result)
}

fn is_lower_hex(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
