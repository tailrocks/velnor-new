// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
};

use velnor_actions_contract::homebrew_adoption::{
    HomebrewAdoptionCandidateInput, HomebrewAdoptionTransition, HomebrewAdoptionTreeChange,
    HomebrewAdoptionTreeFile,
};

use crate::homebrew_adoption_seed_public_deletions::EXPECTED_LEGACY_PATHS;

pub(super) fn assert_candidate_inventory(
    transition: &HomebrewAdoptionTransition,
) -> Result<(), Box<dyn Error>> {
    let accepted = Candidate::for_transition(transition)?;
    assert_eq!(accepted.claude_candidate, b"@AGENTS.md\n");
    assert_eq!(
        accepted.claude_candidate,
        velnor_actions_contract::CLAUDE_MD_POINTER_BODY.as_bytes()
    );
    let evidence = accepted.validate(transition)?;
    let actual = evidence
        .changed_paths
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut expected = BTreeSet::from(EXPECTED_LEGACY_PATHS);
    expected.extend(transition.generated_output_paths.iter().map(String::as_str));
    expected.insert("CLAUDE.md");
    expected.insert(
        velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH,
    );
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 27);

    let mut extra = Candidate::for_transition(transition)?;
    extra.changes.push(deletion(
        "README.md",
        0o100_644,
        "1111111111111111111111111111111111111111",
    ));
    extra
        .changes
        .sort_by(|left, right| left.path.cmp(&right.path));
    assert!(extra.validate(transition).is_err());

    let mut retained = Candidate::for_transition(transition)?;
    let removed = retained.changes[0].clone();
    retained.files.push(HomebrewAdoptionTreeFile {
        path: removed.path.clone(),
        mode: removed
            .old_mode
            .ok_or_else(|| io::Error::other("deletion lacks old mode"))?,
        blob_sha: removed
            .old_blob_sha
            .clone()
            .ok_or_else(|| io::Error::other("deletion lacks old blob"))?,
    });
    assert!(retained.validate(transition).is_err());

    let mut wrong_blob = Candidate::for_transition(transition)?;
    wrong_blob.changes[0].old_blob_sha =
        Some("2222222222222222222222222222222222222222".to_owned());
    assert!(wrong_blob.validate(transition).is_err());

    let mut wrong_mode = Candidate::for_transition(transition)?;
    wrong_mode.changes[0].old_mode = Some(0o100_755);
    assert!(wrong_mode.validate(transition).is_err());
    assert_claude_pointer_candidate(transition)?;
    Ok(())
}

fn assert_claude_pointer_candidate(
    transition: &HomebrewAdoptionTransition,
) -> Result<(), Box<dyn Error>> {
    let accepted = Candidate::for_transition(transition)?;
    assert!(accepted.validate(transition).is_ok());

    for bytes in [
        b"source".as_slice(),
        b"".as_slice(),
        b"@AGENTS.md".as_slice(),
        b"@AGENTS.md\nextra".as_slice(),
    ] {
        let mut altered = accepted.clone();
        altered.claude_output = bytes.to_vec();
        altered.set_claude_candidate(bytes);
        assert!(altered.validate(transition).is_err());
    }

    let mut changed_source = accepted.clone();
    let agents = changed_source
        .files
        .iter_mut()
        .find(|file| file.path == "AGENTS.md")
        .ok_or_else(|| io::Error::other("candidate omitted AGENTS.md"))?;
    agents.blob_sha = "2222222222222222222222222222222222222222".to_owned();
    assert!(changed_source.validate(transition).is_err());

    for mode in [0o100_755, 0o120_000] {
        let mut altered = accepted.clone();
        altered.set_claude_mode(mode);
        assert!(altered.validate(transition).is_err());
    }

    let mut bytes_mismatch = accepted;
    bytes_mismatch.claude_output = b"@AGENTS.md\nextra".to_vec();
    assert!(bytes_mismatch.validate(transition).is_err());
    Ok(())
}

#[derive(Clone)]
struct Candidate {
    files: Vec<HomebrewAdoptionTreeFile>,
    changes: Vec<HomebrewAdoptionTreeChange>,
    generated: BTreeMap<String, Vec<u8>>,
    file_bytes: BTreeMap<String, Vec<u8>>,
    locator: Vec<u8>,
    claude_output: Vec<u8>,
    claude_candidate: Vec<u8>,
}

impl Candidate {
    fn for_transition(transition: &HomebrewAdoptionTransition) -> Result<Self, Box<dyn Error>> {
        let mut candidate = Self {
            files: Vec::new(),
            changes: Vec::new(),
            generated: BTreeMap::new(),
            file_bytes: BTreeMap::new(),
            locator: transition.source_authority_locator.canonical_bytes()?,
            claude_output: velnor_actions_contract::CLAUDE_MD_POINTER_BODY
                .as_bytes()
                .to_vec(),
            claude_candidate: velnor_actions_contract::CLAUDE_MD_POINTER_BODY
                .as_bytes()
                .to_vec(),
        };
        let placeholder = "3333333333333333333333333333333333333333";
        for identity in &transition.deletions {
            candidate
                .changes
                .push(deletion(&identity.path, identity.mode, &identity.blob_sha));
        }
        for inert in &transition.unchanged_inert_files {
            candidate
                .files
                .push(tree_file(&inert.path, inert.mode, &inert.blob_sha));
        }
        candidate.files.push(tree_file(
            "AGENTS.md",
            0o100_644,
            &transition.claude_source_blob_sha,
        ));
        candidate
            .files
            .push(tree_file("CLAUDE.md", 0o100_644, placeholder));
        candidate
            .file_bytes
            .insert("CLAUDE.md".to_owned(), candidate.claude_candidate.clone());
        candidate.file_bytes.insert(
            velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH
                .to_owned(),
            candidate.locator.clone(),
        );
        candidate.changes.push(HomebrewAdoptionTreeChange {
            path: "CLAUDE.md".to_owned(),
            old_mode: Some(0o120_000),
            new_mode: Some(0o100_644),
            old_blob_sha: Some("4444444444444444444444444444444444444444".to_owned()),
            new_blob_sha: Some(placeholder.to_owned()),
            status: "T".to_owned(),
        });
        for path in &transition.generated_output_paths {
            let bytes = path.as_bytes().to_vec();
            candidate.generated.insert(path.clone(), bytes.clone());
            candidate.file_bytes.insert(path.clone(), bytes);
            candidate
                .files
                .push(tree_file(path, 0o100_644, placeholder));
            candidate.changes.push(HomebrewAdoptionTreeChange {
                path: path.clone(),
                old_mode: Some(0o100_644),
                new_mode: Some(0o100_644),
                old_blob_sha: Some("5555555555555555555555555555555555555555".to_owned()),
                new_blob_sha: Some(placeholder.to_owned()),
                status: "M".to_owned(),
            });
        }
        let locator_path =
            velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
        candidate
            .files
            .push(tree_file(locator_path, 0o100_644, placeholder));
        candidate.changes.push(HomebrewAdoptionTreeChange {
            path: locator_path.to_owned(),
            old_mode: None,
            new_mode: Some(0o100_644),
            old_blob_sha: None,
            new_blob_sha: Some(placeholder.to_owned()),
            status: "A".to_owned(),
        });
        candidate
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        candidate
            .changes
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(candidate)
    }

    fn validate(
        &self,
        transition: &HomebrewAdoptionTransition,
    ) -> Result<velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence, Box<dyn Error>>
    {
        Ok(
            transition.validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "cccccccccccccccccccccccccccccccccccccccc",
                ),
                changes: &self.changes,
                candidate_files: &self.files,
                claude: &self.claude_output,
                locator: &self.locator,
                generated_files: &self.generated,
                candidate_file_bytes: &self.file_bytes,
            })?,
        )
    }

    fn set_claude_candidate(&mut self, bytes: &[u8]) {
        self.claude_candidate = bytes.to_vec();
        self.file_bytes
            .insert("CLAUDE.md".to_owned(), self.claude_candidate.clone());
    }

    fn set_claude_mode(&mut self, mode: u32) {
        if let Some(file) = self.files.iter_mut().find(|file| file.path == "CLAUDE.md") {
            file.mode = mode;
        }
        if let Some(change) = self
            .changes
            .iter_mut()
            .find(|change| change.path == "CLAUDE.md")
        {
            change.new_mode = Some(mode);
        }
    }
}

fn tree_file(path: &str, mode: u32, blob_sha: &str) -> HomebrewAdoptionTreeFile {
    HomebrewAdoptionTreeFile {
        path: path.to_owned(),
        mode,
        blob_sha: blob_sha.to_owned(),
    }
}

fn deletion(path: &str, mode: u32, blob_sha: &str) -> HomebrewAdoptionTreeChange {
    HomebrewAdoptionTreeChange {
        path: path.to_owned(),
        old_mode: Some(mode),
        new_mode: None,
        old_blob_sha: Some(blob_sha.to_owned()),
        new_blob_sha: None,
        status: "D".to_owned(),
    }
}
