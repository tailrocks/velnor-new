// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

#[test]
fn a_cargo_feature_that_is_stable_does_not_need_nightly() {
    let mut stable = files();
    stable.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "cargo-features = [\"edition2024\"]\n[package]\nname = \"app\"\n".to_owned(),
    );
    assert_eq!(refusal(&stable), None);
}

#[test]
fn a_nightly_manifest_is_checked_only_where_a_release_root_reaches_it() {
    let mut files = files();
    files.release_toml = Some("roots = [\"app\"]\n".to_owned());
    let plz = files.release_plz_toml.remove(".").unwrap_or_default();
    files.release_plz_toml.insert("app".to_owned(), plz);
    let nightly = "cargo-features = [\"test-dummy-unstable\"]\n";
    files
        .cargo_manifest_texts
        .insert("experiments/Cargo.toml".to_owned(), nightly.to_owned());
    assert_eq!(refusal(&files), None);
    files
        .cargo_manifest_texts
        .insert("app/member/Cargo.toml".to_owned(), nightly.to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::NightlyOnly { file, .. }) if file == "app/member/Cargo.toml"
    ));
}
