// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, BTreeSet, ContractError, HomebrewAdoptionCandidateInput, HomebrewAdoptionEvidence,
    HomebrewAdoptionTransition, HomebrewAdoptionTreeChange, HomebrewAdoptionTreeFile, change_map,
    full_sha, invalid, tree_map,
};

impl HomebrewAdoptionTransition {
    pub fn validate_candidate_tree(
        &self,
        input: HomebrewAdoptionCandidateInput<'_>,
    ) -> Result<HomebrewAdoptionEvidence, ContractError> {
        self.validate_candidate_context(&input)?;
        let candidate = tree_map(input.candidate_files)?;
        validate_candidate_modes(self, &candidate)?;
        let changes = change_map(input.changes)?;
        validate_change_set(self, &candidate, &changes)?;
        let locator = self.source_authority_locator.canonical_bytes()?;
        if input.locator != locator || input.claude != crate::CLAUDE_MD_POINTER_BODY.as_bytes() {
            return Err(invalid("homebrew_adoption_expected_bytes"));
        }
        validate_generated_bytes(self, &input, &candidate, &changes, &locator)?;
        Ok(HomebrewAdoptionEvidence {
            schema: 1,
            transition_id: self.transition_id.clone(),
            transition_digest: self.digest()?,
            base_sha: input.base.0.to_owned(),
            base_tree: input.base.1.to_owned(),
            head_sha: input.head.0.to_owned(),
            head_tree: input.head.1.to_owned(),
            changed_paths: changes.keys().cloned().collect(),
        })
    }

    fn validate_candidate_context(
        &self,
        input: &HomebrewAdoptionCandidateInput<'_>,
    ) -> Result<(), ContractError> {
        self.validate()?;
        if input.base.0 != self.base_sha
            || input.base.1 != self.base_tree
            || !full_sha(input.head.0)
            || !full_sha(input.head.1)
            || input.head.0 == input.base.0
            || input
                .generated_files
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != self
                    .generated_output_paths
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
        {
            return Err(invalid("homebrew_adoption_base_or_head"));
        }
        Ok(())
    }
}

fn validate_candidate_modes(
    transition: &HomebrewAdoptionTransition,
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
) -> Result<(), ContractError> {
    if candidate.values().any(|entry| !is_regular_mode(entry.mode))
        || candidate.get("AGENTS.md").is_none_or(|entry| {
            entry.mode != 0o100_644 || entry.blob_sha != transition.claude_source_blob_sha
        })
        || candidate
            .get("CLAUDE.md")
            .is_none_or(|entry| entry.mode != 0o100_644)
    {
        return Err(invalid("homebrew_adoption_claude_modes"));
    }
    Ok(())
}

fn validate_change_set(
    transition: &HomebrewAdoptionTransition,
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
) -> Result<(), ContractError> {
    if changes.keys().cloned().collect::<BTreeSet<_>>() != transition.changed_path_set() {
        return Err(invalid("homebrew_adoption_changed_path_set"));
    }
    validate_candidate_matches_diff(candidate, changes)?;
    validate_deletions(transition, candidate, changes)?;
    validate_inert_files(transition, candidate, changes)?;
    validate_required_additions(transition, changes)
}

fn validate_candidate_matches_diff(
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
) -> Result<(), ContractError> {
    for change in changes.values() {
        match change.new_blob_sha.as_deref() {
            Some(sha)
                if candidate.get(&change.path).is_none_or(|entry| {
                    entry.blob_sha != sha || Some(entry.mode) != change.new_mode
                }) =>
            {
                return Err(invalid("homebrew_adoption_diff_candidate_mismatch"));
            }
            None if candidate.contains_key(&change.path) => {
                return Err(invalid("homebrew_adoption_diff_candidate_mismatch"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_deletions(
    transition: &HomebrewAdoptionTransition,
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
) -> Result<(), ContractError> {
    for expected in &transition.deletions {
        let actual = changes
            .get(&expected.path)
            .ok_or_else(|| invalid("homebrew_adoption_deletion_mismatch"))?;
        if actual.status != "D"
            || actual.old_mode != Some(expected.mode)
            || actual.old_blob_sha.as_deref() != Some(expected.blob_sha.as_str())
            || actual.new_mode.is_some()
            || actual.new_blob_sha.is_some()
            || candidate.contains_key(&expected.path)
        {
            return Err(invalid("homebrew_adoption_deletion_mismatch"));
        }
    }
    Ok(())
}

fn validate_inert_files(
    transition: &HomebrewAdoptionTransition,
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
) -> Result<(), ContractError> {
    for expected in &transition.unchanged_inert_files {
        if changes.contains_key(&expected.path)
            || candidate.get(&expected.path).is_none_or(|actual| {
                actual.mode != expected.mode || actual.blob_sha != expected.blob_sha
            })
        {
            return Err(invalid("homebrew_adoption_inert_file_changed"));
        }
    }
    Ok(())
}

fn validate_required_additions(
    transition: &HomebrewAdoptionTransition,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
) -> Result<(), ContractError> {
    let locator = crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
    for (path, status) in [("CLAUDE.md", "T"), (locator, "A")] {
        let change = changes
            .get(path)
            .ok_or_else(|| invalid("homebrew_adoption_required_change_missing"))?;
        if change.status != status || !valid_added_identity(change) {
            return Err(invalid("homebrew_adoption_required_change_invalid"));
        }
    }
    for path in &transition.generated_output_paths {
        let change = changes
            .get(path)
            .ok_or_else(|| invalid("homebrew_adoption_generated_change_missing"))?;
        if !matches!(change.status.as_str(), "A" | "M") || !valid_added_identity(change) {
            return Err(invalid("homebrew_adoption_generated_change_invalid"));
        }
    }
    Ok(())
}

fn valid_added_identity(change: &HomebrewAdoptionTreeChange) -> bool {
    change.new_mode == Some(0o100_644) && change.new_blob_sha.as_deref().is_some_and(full_sha)
}

fn validate_generated_bytes(
    transition: &HomebrewAdoptionTransition,
    input: &HomebrewAdoptionCandidateInput<'_>,
    candidate: &BTreeMap<String, HomebrewAdoptionTreeFile>,
    changes: &BTreeMap<String, HomebrewAdoptionTreeChange>,
    locator: &[u8],
) -> Result<(), ContractError> {
    let mut expected = input.generated_files.clone();
    expected.insert("CLAUDE.md".to_owned(), input.claude.to_vec());
    expected.insert(
        crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH.to_owned(),
        locator.to_vec(),
    );
    for (path, bytes) in &expected {
        let change = changes
            .get(path)
            .ok_or_else(|| invalid("homebrew_adoption_generated_change_missing"))?;
        if input.candidate_file_bytes.get(path) != Some(bytes)
            || candidate
                .get(path)
                .is_none_or(|entry| entry.mode != 0o100_644)
            || change
                .new_blob_sha
                .as_deref()
                .is_none_or(|sha| !full_sha(sha))
        {
            return Err(invalid("homebrew_adoption_generated_file_mismatch"));
        }
    }
    if expected.len() != transition.generated_output_paths.len() + 2 {
        return Err(invalid("homebrew_adoption_generated_file_duplicate"));
    }
    Ok(())
}

fn is_regular_mode(mode: u32) -> bool {
    matches!(mode, 0o100_644 | 0o100_755)
}
