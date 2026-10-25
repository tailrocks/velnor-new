//! Protected, data-only admission for one exact Homebrew tap seed transition.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, config::NativeHomebrewOperation,
    homebrew_authority::HomebrewSourceAuthorityLocator, vcs::is_safe_github_repository,
};

#[path = "homebrew_adoption_candidate.rs"]
mod candidate;
#[cfg(test)]
#[path = "homebrew_adoption_tests.rs"]
mod tests;
#[path = "homebrew_adoption_validation.rs"]
mod validation;
use validation::{
    change_map, full_sha, invalid, invalid_owned, safe_repo_path, safe_transition_id,
    stable_semver, strict_sorted_unique, tree_map, validate_identities,
};

/// Closed schema identifier for reviewed Homebrew adoption transitions.
pub const HOMEBREW_ADOPTION_TRANSITION_SCHEMA_V1: &str = "velnor.homebrew-adoption-transition.v1";
/// Maximum serialized transition record size.
pub const MAX_HOMEBREW_ADOPTION_TRANSITION_BYTES: usize = 128 * 1024;

/// One exact protected base and its permitted seed change set.
///
/// This is policy data. Candidate files never supply or extend this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionTransition {
    /// Closed schema discriminator.
    pub schema: String,
    /// Protected SDK catalog key.
    pub transition_id: String,
    /// Registered tap repository slug.
    pub tap_repository: String,
    /// GitHub numeric tap repository identity.
    pub tap_repository_id: u64,
    /// Exact starting protected main commit.
    pub base_sha: String,
    /// Exact starting protected main tree.
    pub base_tree: String,
    /// Fixed reviewed semantic transition template version.
    pub template_version: u32,
    /// Minimum authenticated SDK release that implements this transition.
    pub minimum_sdk_version: String,
    /// Closed SDK capability required by the protected seed workflow.
    pub required_sdk_capability: String,
    /// Exact source-authority locator required in the candidate tree.
    pub source_authority_locator: HomebrewSourceAuthorityLocator,
    /// Exact regular source file copied to `CLAUDE.md`.
    pub claude_source_path: String,
    /// Exact Git blob for the source file copied to `CLAUDE.md`.
    pub claude_source_blob_sha: String,
    /// Exact deleted legacy helper identities.
    pub deletions: Vec<HomebrewAdoptionFileIdentity>,
    /// Exact untouched inert migration-input identities.
    pub unchanged_inert_files: Vec<HomebrewAdoptionFileIdentity>,
    /// Paths owned by the SDK's trusted renderer for this transition.
    pub generated_output_paths: Vec<String>,
    /// Exact seed obligations. This closed transition requires tree and adoption.
    pub required_operations: Vec<NativeHomebrewOperation>,
}

/// One exact regular-file identity in a protected transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionFileIdentity {
    /// Safe repository-relative file path.
    pub path: String,
    /// Git regular-file mode.
    pub mode: u32,
    /// Full lowercase Git blob SHA-1.
    pub blob_sha: String,
}

/// Candidate tree identity used by the pure transition validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewAdoptionTreeFile {
    /// Safe repository-relative file path.
    pub path: String,
    /// Git regular-file mode.
    pub mode: u32,
    /// Full lowercase Git blob SHA-1.
    pub blob_sha: String,
}

/// One complete base-to-candidate Git diff record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewAdoptionTreeChange {
    /// Safe repository-relative path.
    pub path: String,
    /// Prior Git mode; `None` for additions.
    pub old_mode: Option<u32>,
    /// Candidate Git mode; `None` for deletions.
    pub new_mode: Option<u32>,
    /// Prior Git blob; `None` for additions.
    pub old_blob_sha: Option<String>,
    /// Candidate Git blob; `None` for deletions.
    pub new_blob_sha: Option<String>,
    /// Closed raw diff status (`A`, `D`, `M`, or `T`).
    pub status: String,
}

/// Complete authenticated inputs to candidate-tree validation.
#[derive(Debug, Clone, Copy)]
pub struct HomebrewAdoptionCandidateInput<'a> {
    /// Exact protected base commit and tree.
    pub base: (&'a str, &'a str),
    /// Exact candidate head commit and tree.
    pub head: (&'a str, &'a str),
    /// Complete diff and candidate regular/symlink tree records.
    pub changes: &'a [HomebrewAdoptionTreeChange],
    /// Candidate tree entries authenticated from Git.
    pub candidate_files: &'a [HomebrewAdoptionTreeFile],
    /// Protected AGENTS bytes and candidate CLAUDE bytes.
    pub claude: (&'a [u8], &'a [u8]),
    /// Candidate source-authority locator bytes.
    pub locator: &'a [u8],
    /// Renderer-owned expected output files from authenticated SDK inputs.
    pub generated_files: &'a BTreeMap<String, Vec<u8>>,
    /// Candidate file bytes read from exact Git blobs.
    pub candidate_file_bytes: &'a BTreeMap<String, Vec<u8>>,
}

/// Successful exact seed-transition evidence, with no publication authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAdoptionEvidence {
    /// Evidence schema version.
    pub schema: u32,
    /// Protected transition identifier.
    pub transition_id: String,
    /// Digest of the canonical protected transition record.
    pub transition_digest: String,
    /// Exact authenticated starting commit and tree.
    pub base_sha: String,
    /// Exact authenticated starting tree.
    pub base_tree: String,
    /// Exact candidate commit and tree.
    pub head_sha: String,
    /// Exact candidate tree.
    pub head_tree: String,
    /// Sorted exact changed paths.
    pub changed_paths: Vec<String>,
}

impl HomebrewAdoptionTransition {
    /// Validate all closed fields and cross-field path rules.
    ///
    /// # Errors
    /// Returns an error when a closed field or cross-field invariant is invalid.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != HOMEBREW_ADOPTION_TRANSITION_SCHEMA_V1
            || !safe_transition_id(&self.transition_id)
            || !is_safe_github_repository(&self.tap_repository)
            || self.tap_repository_id == 0
            || !full_sha(&self.base_sha)
            || !full_sha(&self.base_tree)
            || self.template_version != 1
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
        if self.generated_output_paths.is_empty()
            || !strict_sorted_unique(&self.generated_output_paths)
            || self.generated_output_paths.iter().any(|path| {
                !safe_repo_path(path)
                    || !(path.starts_with(".github/workflows/")
                        || path == ".velnor/config.toml"
                        || path == "mise.toml"
                        || path == "mise.lock")
            })
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

    /// Canonical, computed digest. No digest field is stored in policy.
    ///
    /// # Errors
    /// Returns an error if the transition is invalid or cannot be canonically encoded.
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

/// Parse one bounded, canonical, closed transition record.
///
/// # Errors
/// Returns an error when bytes are invalid UTF-8, non-canonical, oversized, or
/// fail the transition schema and invariant checks.
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
