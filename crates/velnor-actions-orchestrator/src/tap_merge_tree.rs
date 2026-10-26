// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    expected_base_formula_sha256: &str,
) -> Result<(), OrchestratorError> {
    let base = commit_tree(api, &candidate.policy.repository, base_sha)?;
    let head = commit_tree(api, &candidate.policy.repository, head_sha)?;
    let base_entries = recursive_tree(api, &candidate.policy.repository, &base)?;
    let head_entries = recursive_tree(api, &candidate.policy.repository, &head)?;
    let formula = candidate.formula_path.as_str();
    validate_inventory_delta(&base_entries, &head_entries, formula)?;
    let base_formula =
        super::observation::contents(api, &candidate.policy.repository, formula, base_sha)?;
    if super::candidate::sha256(&base_formula) != expected_base_formula_sha256 {
        return Err(internal("tap_candidate_base_digest_mismatch"));
    }
    let head_formula =
        super::observation::contents(api, &candidate.policy.repository, formula, head_sha)?;
    if head_formula != expected_formula {
        return Err(internal("tap_merge_formula_head_mismatch"));
    }
    validate_formula_at_base(candidate, base_sha, &base_formula, expected_formula)
}

fn map_inventory(
    entries: &BTreeMap<String, (String, String, String)>,
) -> Vec<crate::homebrew::tree_validation::Entry<'_>> {
    entries
        .iter()
        .map(
            |(path, (mode, kind, sha))| crate::homebrew::tree_validation::Entry {
                path,
                mode,
                kind,
                sha,
            },
        )
        .collect()
}

fn validate_inventory_delta(
    base: &BTreeMap<String, (String, String, String)>,
    head: &BTreeMap<String, (String, String, String)>,
    formula: &str,
) -> Result<(), OrchestratorError> {
    let base_inventory = map_inventory(base);
    let head_inventory = map_inventory(head);
    crate::homebrew::tree_validation::validate_formula_delta(
        &base_inventory,
        &head_inventory,
        formula,
        TREE_LIMIT,
    )
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
    parse_recursive_tree(&response, sha)
}

fn parse_recursive_tree(
    response: &Value,
    sha: &str,
) -> Result<BTreeMap<String, (String, String, String)>, OrchestratorError> {
    let entries = response["tree"]
        .as_array()
        .filter(|entries| entries.len() <= TREE_LIMIT)
        .ok_or_else(|| internal("tap_merge_tree_unreadable"))?;
    if response["sha"].as_str() != Some(sha) || response["truncated"].as_bool() != Some(false) {
        return Err(internal("tap_merge_tree_truncated"));
    }
    let mut result = BTreeMap::new();
    let mut inventory = Vec::with_capacity(entries.len());
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
        inventory.push(crate::homebrew::tree_validation::Entry {
            path,
            mode,
            kind: object_type,
            sha: object_sha,
        });
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
    crate::homebrew::tree_validation::validate(&inventory, TREE_LIMIT)?;
    Ok(result)
}

fn is_lower_hex(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
