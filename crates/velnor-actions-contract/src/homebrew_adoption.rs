// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, config::NativeHomebrewOperation,
    homebrew_authority::HomebrewSourceAuthorityLocator, vcs::is_safe_github_repository,
};

#[path = "homebrew_adoption_candidate.rs"]
mod candidate;

#[path = "homebrew_adoption_validation.rs"]
mod validation;
use validation::{
    change_map, full_sha, invalid, invalid_owned, safe_repo_path, safe_transition_id,
    stable_semver, strict_sorted_unique, tree_map, validate_identities,
};

pub const HOMEBREW_ADOPTION_TRANSITION_SCHEMA_V1: &str = "velnor.homebrew-adoption-transition.v1";

pub const HOMEBREW_ADOPTION_SEED_V2_OUTPUT_PATHS: &[&str] = &[
    ".github/workflows/ci.yml",
    ".velnor/config.toml",
    ".velnor/release-manifest.json",
    "mise.lock",
    "mise.toml",
];

pub const MAX_HOMEBREW_ADOPTION_TRANSITION_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionTransition {
    pub schema: String,

    pub transition_id: String,

    pub tap_repository: String,

    pub tap_repository_id: u64,

    pub base_sha: String,

    pub base_tree: String,

    pub template_version: u32,

    pub minimum_sdk_version: String,

    pub required_sdk_capability: String,

    pub source_authority_locator: HomebrewSourceAuthorityLocator,

    pub claude_source_path: String,

    pub claude_source_blob_sha: String,

    pub deletions: Vec<HomebrewAdoptionFileIdentity>,

    pub unchanged_inert_files: Vec<HomebrewAdoptionFileIdentity>,

    pub generated_output_paths: Vec<String>,

    pub required_operations: Vec<NativeHomebrewOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionFileIdentity {
    pub path: String,

    pub mode: u32,

    pub blob_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewAdoptionTreeFile {
    pub path: String,

    pub mode: u32,

    pub blob_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewAdoptionTreeChange {
    pub path: String,

    pub old_mode: Option<u32>,

    pub new_mode: Option<u32>,

    pub old_blob_sha: Option<String>,

    pub new_blob_sha: Option<String>,

    pub status: String,
}

#[derive(Debug, Clone, Copy)]
pub struct HomebrewAdoptionCandidateInput<'a> {
    pub base: (&'a str, &'a str),

    pub head: (&'a str, &'a str),

    pub changes: &'a [HomebrewAdoptionTreeChange],

    pub candidate_files: &'a [HomebrewAdoptionTreeFile],

    pub claude: (&'a [u8], &'a [u8]),

    pub locator: &'a [u8],

    pub generated_files: &'a BTreeMap<String, Vec<u8>>,

    pub candidate_file_bytes: &'a BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionEvidence {
    pub schema: u32,

    pub transition_id: String,

    pub transition_digest: String,

    pub base_sha: String,

    pub base_tree: String,

    pub head_sha: String,

    pub head_tree: String,

    pub changed_paths: Vec<String>,
}

impl HomebrewAdoptionTransition {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != HOMEBREW_ADOPTION_TRANSITION_SCHEMA_V1
            || !safe_transition_id(&self.transition_id)
            || !is_safe_github_repository(&self.tap_repository)
            || self.tap_repository_id == 0
            || !full_sha(&self.base_sha)
            || !full_sha(&self.base_tree)
            || self.template_version != 2
            || !stable_semver(&self.minimum_sdk_version)
            || self.required_sdk_capability != "homebrew-adoption-seed-v1"
            || self.claude_source_path != "AGENTS.md"
            || !full_sha(&self.claude_source_blob_sha)
        {
            return Err(invalid("homebrew_adoption_transition_identity"));
        }
        self.source_authority_locator.validate()?;
        if self.source_authority_locator.repository.is_empty()
            || self.source_authority_locator.repository_id == 0
        {
            return Err(invalid("homebrew_adoption_locator_identity"));
        }

        validate_identities(&self.deletions, "homebrew_adoption_deletions")?;
        validate_identities(&self.unchanged_inert_files, "homebrew_adoption_inert_files")?;
        if self.deletions.is_empty()
            || self
                .deletions
                .iter()
                .any(|entry| !entry.path.starts_with("scripts/"))
        {
            return Err(invalid("homebrew_adoption_deletion_inventory"));
        }
        if self.unchanged_inert_files.is_empty() {
            return Err(invalid("homebrew_adoption_inert_inventory"));
        }
        if !strict_sorted_unique(&self.generated_output_paths)
            || !self
                .generated_output_paths
                .iter()
                .map(String::as_str)
                .eq(HOMEBREW_ADOPTION_SEED_V2_OUTPUT_PATHS.iter().copied())
            || self
                .generated_output_paths
                .iter()
                .any(|path| !safe_repo_path(path))
        {
            return Err(invalid("homebrew_adoption_generated_paths"));
        }
        let required = [
            NativeHomebrewOperation::TapTree,
            NativeHomebrewOperation::AdoptionContract,
        ];
        if self.required_operations.as_slice() != required {
            return Err(invalid("homebrew_adoption_required_operations"));
        }
        let changed = self.changed_path_set();
        let inert: BTreeSet<_> = self
            .unchanged_inert_files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        let deleted: BTreeSet<_> = self
            .deletions
            .iter()
            .map(|file| file.path.clone())
            .collect();
        let generated: BTreeSet<_> = self.generated_output_paths.iter().cloned().collect();
        if !inert.is_disjoint(&changed)
            || generated.contains("CLAUDE.md")
            || generated.contains("AGENTS.md")
            || generated.contains(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
            || !generated.is_disjoint(&deleted)
            || !generated.is_disjoint(&inert)
        {
            return Err(invalid("homebrew_adoption_changed_inert_path"));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, ContractError> {
        self.validate()?;
        let bytes = crate::canonical_json_bytes(self)?;
        Ok(crate::digest_b3(&bytes))
    }

    pub(crate) fn changed_path_set(&self) -> BTreeSet<String> {
        let mut changed = BTreeSet::new();
        changed.extend(self.deletions.iter().map(|file| file.path.clone()));
        changed.insert("CLAUDE.md".to_owned());
        changed
            .insert(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH.to_owned());
        changed.extend(self.generated_output_paths.iter().cloned());
        changed
    }
}

pub fn parse_homebrew_adoption_transition(
    bytes: &[u8],
) -> Result<HomebrewAdoptionTransition, ContractError> {
    let text = std::str::from_utf8(bytes).map_err(|error| invalid_owned(error.to_string()))?;
    let value = crate::strict_json::parse_strict_json_with_limit(
        text,
        MAX_HOMEBREW_ADOPTION_TRANSITION_BYTES,
    )?;
    let transition: HomebrewAdoptionTransition =
        serde_json::from_value(value).map_err(|error| invalid_owned(error.to_string()))?;
    transition.validate()?;
    if crate::canonical_json_bytes(&transition)? != bytes {
        return Err(invalid("homebrew_adoption_transition_not_canonical"));
    }
    Ok(transition)
}
