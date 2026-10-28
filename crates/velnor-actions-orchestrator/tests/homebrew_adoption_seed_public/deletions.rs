// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, io};

use velnor_actions_contract::homebrew_adoption::{
    HomebrewAdoptionTransition, parse_homebrew_adoption_transition,
};

pub(super) const EXPECTED_LEGACY_PATHS: [&str; 19] = [
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
    crate::homebrew_adoption_seed_public_candidate::assert_candidate_inventory(&transition)?;
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
    assert_eq!(transition.minimum_sdk_version, "0.1.66");
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
