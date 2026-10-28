// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
};

use velnor_actions_contract::homebrew_adoption::{
    HomebrewAdoptionCandidateInput, HomebrewAdoptionTransition, HomebrewAdoptionTreeChange,
    HomebrewAdoptionTreeFile, parse_homebrew_adoption_transition,
};

const EXPECTED_LEGACY_PATHS: [&str; 19] = [
    ".github/actions/u26/action.yml",
    ".github/actions/velnor-tool-seed/action.yml",
    ".github/actions/velnor-tools-cache-restore/action.yml",
    ".github/actions/velnor-tools-prelude-u26/action.yml",
    ".github/actions/velnor-verify-jactionlint/action.yml",
    ".github/actions/velnor-verify-restored-rust/action.yml",
    ".github/actions/velnor-verify-rust-manifest/action.yml",
    ".github/scripts/velnor-tools-cache-identity.sh",
    ".github/jactionlint.yaml",
    ".github/workflows/cache-prune.yml",
    "scripts/homebrew_install_policy.py",
    "scripts/package-update.py",
    "scripts/package-update.sh",
    "scripts/package_update_validation.py",
    "scripts/test-homebrew-install.sh",
    "scripts/test-package-update.sh",
    "scripts/test_homebrew_formulary_keg.rb",
    "scripts/test_homebrew_install_policy.py",
    "scripts/test_package_update.py",
];

const NEW_LEGACY_IDENTITIES: [(&str, &str); 10] = [
    (
        ".github/actions/u26/action.yml",
        "a9c967b569da832c684cfc2aa58eb5f428270707",
    ),
    (
        ".github/actions/velnor-tool-seed/action.yml",
        "27f47bcc51d520bc9d34dcfaa8c9e0ef3c25828e",
    ),
    (
        ".github/actions/velnor-tools-cache-restore/action.yml",
        "5e5bcdc6ede01effc48553a9ca9455e104460fc5",
    ),
    (
        ".github/actions/velnor-tools-prelude-u26/action.yml",
        "581a70f42e6831384681cd24d0950a4fbb755de4",
    ),
    (
        ".github/actions/velnor-verify-jactionlint/action.yml",
        "7c3531af09cbdcd3421b59bb7bbf84e74e59d47a",
    ),
    (
        ".github/actions/velnor-verify-restored-rust/action.yml",
        "a84e33502e294d8c47c322fcad5422409d45f16c",
    ),
    (
        ".github/actions/velnor-verify-rust-manifest/action.yml",
        "b579164d33c2641e1f9ccad378557285d05fa9e8",
    ),
    (
        ".github/scripts/velnor-tools-cache-identity.sh",
        "b0b3fb1ebabc599dad940465e58c231565d42fb0",
    ),
    (
        ".github/jactionlint.yaml",
        "f8549ccba3f6d2b345ec3db07b1f918b2e17ba70",
    ),
    (
        ".github/workflows/cache-prune.yml",
        "5fcab017f0388a7aa6ce18ae5d8694d2bf34eff0",
    ),
];

#[test]
fn public_seed_transition_binds_exact_legacy_inventory_and_candidate() -> Result<(), Box<dyn Error>>
{
    let bytes = include_bytes!("../../../../.velnor/qualification/homebrew/jackin/transition.json");
    let transition = parse_homebrew_adoption_transition(bytes)?;
    let mut noncanonical = bytes.to_vec();
    noncanonical.push(b'\n');
    assert!(parse_homebrew_adoption_transition(&noncanonical).is_err());
    assert_exact_inventory(&transition)?;
    assert_transition_digest_binding(&transition)?;
    assert_candidate_inventory(&transition)?;
    assert_other_github_deletion_classes_rejected(&transition);
    Ok(())
}

fn assert_exact_inventory(transition: &HomebrewAdoptionTransition) -> Result<(), Box<dyn Error>> {
    let paths = transition
        .deletions
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(paths, BTreeSet::from(EXPECTED_LEGACY_PATHS));
    assert_eq!(transition.deletions.len(), 19);
    for (path, blob_sha) in NEW_LEGACY_IDENTITIES {
        let entry = transition
            .deletions
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(|| io::Error::other(format!("transition omitted {path}")))?;
        assert_eq!(entry.mode, 0o100_644, "unexpected mode for {path}");
        assert_eq!(entry.blob_sha, blob_sha, "unexpected blob for {path}");
    }
    assert_eq!(transition.minimum_sdk_version, "0.1.65");
    transition.validate()?;
    Ok(())
}

fn assert_transition_digest_binding(
    transition: &HomebrewAdoptionTransition,
) -> Result<(), Box<dyn Error>> {
    let policy: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../.velnor/qualification/homebrew/jackin/cutover-author-policy.json"
    ))?;
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&policy)?,
        include_bytes!(
            "../../../../.velnor/qualification/homebrew/jackin/cutover-author-policy.json"
        )
    );
    let digest = transition.digest()?;
    assert_eq!(
        policy
            .get("transition_digest")
            .and_then(serde_json::Value::as_str),
        Some(digest.as_str())
    );
    Ok(())
}

fn assert_candidate_inventory(
    transition: &HomebrewAdoptionTransition,
) -> Result<(), Box<dyn Error>> {
    let accepted = Candidate::for_transition(transition)?;
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
    Ok(())
}

fn assert_other_github_deletion_classes_rejected(transition: &HomebrewAdoptionTransition) {
    for path in [
        ".github/workflows/unrelated.yml",
        ".github/workflows/cache-prune-extra.yml",
        ".github/workflows/nested/cache-prune.yml",
        ".github/jactionlint.yml",
        ".github/jactionlint.yaml.bak",
        ".github/actions/unlisted/extra.yml",
        ".github/actions/example/nested/action.yml",
        ".github/actions/../action.yml",
        ".github/scripts/unlisted.sh",
    ] {
        let mut altered = transition.clone();
        path.clone_into(&mut altered.deletions[0].path);
        altered
            .deletions
            .sort_by(|left, right| left.path.cmp(&right.path));
        assert!(altered.validate().is_err(), "accepted deletion {path}");
    }
}

struct Candidate {
    files: Vec<HomebrewAdoptionTreeFile>,
    changes: Vec<HomebrewAdoptionTreeChange>,
    generated: BTreeMap<String, Vec<u8>>,
    file_bytes: BTreeMap<String, Vec<u8>>,
    locator: Vec<u8>,
}

impl Candidate {
    fn for_transition(transition: &HomebrewAdoptionTransition) -> Result<Self, Box<dyn Error>> {
        let mut candidate = Self {
            files: Vec::new(),
            changes: Vec::new(),
            generated: BTreeMap::new(),
            file_bytes: BTreeMap::new(),
            locator: transition.source_authority_locator.canonical_bytes()?,
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
            .insert("CLAUDE.md".to_owned(), b"source".to_vec());
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
        let source = b"source";
        Ok(
            transition.validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "cccccccccccccccccccccccccccccccccccccccc",
                ),
                changes: &self.changes,
                candidate_files: &self.files,
                claude: (source, source),
                locator: &self.locator,
                generated_files: &self.generated,
                candidate_file_bytes: &self.file_bytes,
            })?,
        )
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
