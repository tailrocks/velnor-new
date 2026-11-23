// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, WORKFLOW_PATH};

use crate::facts::TestResult;
use crate::private_sources::{SECTION, private_repo};

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const RENDERED_DIGEST: &str = "1d4af09a6b61f249b7a314eb2b0bf09b83d9afee34633114a57e276f103412ba";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn assets() -> String {
    format!(
        "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '{SHA}'\ndest = 'tests/fixtures/dataset'\n"
    )
}

fn warning(helper: &str) -> String {
    format!(
        "[private_sources] / release assets with helper {helper} mint the App token after repository-controlled steps; upgrade the helper to 0.1.111 or later"
    )
}

fn minted_notes(notes: &[String]) -> Vec<&String> {
    notes
        .iter()
        .filter(|note| note.contains("mint the App token"))
        .collect()
}

#[test]
fn an_older_helper_with_private_sources_is_warned_about_the_token_order() -> TestResult {
    let repo = private_repo(ROOTS, SECTION, "0.1.110")?;
    let (_, notes) = expected_with_notes(&repo)?;
    assert_eq!(minted_notes(&notes), [&warning("0.1.110")], "{notes:?}");
    Ok(())
}

#[test]
fn an_older_helper_with_release_assets_is_warned_about_the_token_order() -> TestResult {
    let repo = private_repo(ROOTS, &format!("{SECTION}{}", assets()), "0.1.110")?;
    let (_, notes) = expected_with_notes(&repo)?;
    assert_eq!(minted_notes(&notes), [&warning("0.1.110")], "{notes:?}");
    Ok(())
}

#[test]
fn an_older_helper_without_private_sources_or_release_assets_is_not_warned_about() -> TestResult {
    let repo = private_repo(ROOTS, "", "0.1.110")?;
    let (_, notes) = expected_with_notes(&repo)?;
    assert!(minted_notes(&notes).is_empty(), "{notes:?}");
    Ok(())
}

#[test]
fn the_helper_that_mints_first_is_not_warned_about() -> TestResult {
    let repo = private_repo(ROOTS, &format!("{SECTION}{}", assets()), "0.1.111")?;
    let (_, notes) = expected_with_notes(&repo)?;
    assert!(minted_notes(&notes).is_empty(), "{notes:?}");
    Ok(())
}

#[test]
fn a_helper_after_the_one_that_mints_first_is_not_warned_about() -> TestResult {
    let repo = private_repo(ROOTS, &format!("{SECTION}{}", assets()), "0.1.112")?;
    let (_, notes) = expected_with_notes(&repo)?;
    assert!(minted_notes(&notes).is_empty(), "{notes:?}");
    Ok(())
}

#[test]
fn the_warning_changes_neither_the_rendered_files_nor_the_generation() -> TestResult {
    let repo = private_repo(ROOTS, SECTION, "0.1.110")?;
    let (files, notes) = expected_with_notes(&repo)?;
    assert_eq!(minted_notes(&notes).len(), 1, "{notes:?}");
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "name: CI"),
        "{workflow}"
    );
    for step in ["cargo-fmt", "cargo-clippy", "cargo-nextest", "mbx"] {
        assert!(workflow.contains(step), "{step} missing from {workflow}");
    }
    assert_eq!(rendered_digest(&files), RENDERED_DIGEST);
    Ok(())
}

fn rendered_digest(files: &BTreeMap<RepoPath, String>) -> String {
    let mut hasher = Sha256::new();
    for (path, text) in files {
        hasher.update(path.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(text.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}
