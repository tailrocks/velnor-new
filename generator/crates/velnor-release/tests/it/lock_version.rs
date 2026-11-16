// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{PLZ, TestResult, files, plz_edited, refusal};

fn manifest(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")
}

#[test]
fn a_release_false_package_with_a_lock_version_does_not_block_generation() -> TestResult {
    let mut files = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"skip\"\nrelease = false",
    )?;
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("app", "1.2.3"));
    files
        .cargo_manifest_texts
        .insert("skip/Cargo.toml".to_owned(), manifest("skip", "1.2.3.lock"));
    assert_eq!(refusal(&files), None);
    files
        .cargo_manifest_texts
        .insert("skip/Cargo.toml".to_owned(), manifest("app", "1.2.3.lock"));
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::VersionEndsInLock(..))
    ));
    Ok(())
}

#[test]
fn an_unparsable_manifest_under_a_release_root_fails_and_names_the_file() {
    let mut files = files();
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("app", "1.2.3"));
    files.cargo_manifest_texts.insert(
        "broken/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion = \n".to_owned(),
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::InvalidToml { file, .. }) if file == "broken/Cargo.toml"),
        "{error:?}"
    );
}

#[test]
fn a_member_root_inheriting_a_lock_workspace_version_is_refused() {
    let mut files = files();
    files.release_toml = Some("roots = [\"member\"]\n".to_owned());
    files.release_plz_toml.clear();
    files
        .release_plz_toml
        .insert("member".to_owned(), PLZ.to_owned());
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"member\"]\n[workspace.package]\nversion = \"1.2.3.lock\"\n"
            .to_owned(),
    );
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n".to_owned(),
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::VersionEndsInLock(path, _)) if path == "member/Cargo.toml"),
        "{error:?}"
    );
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"member\"]\n[workspace.package]\nversion = \"1.2.3\"\n"
            .to_owned(),
    );
    assert_eq!(refusal(&files), None);
}

#[test]
fn a_nested_non_member_workspace_does_not_supply_the_inherited_version() {
    let mut files = files();
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"tools/app\"]\n[workspace.package]\nversion = \"1.2.3\"\n"
            .to_owned(),
    );
    files.cargo_manifest_texts.insert(
        "tools/Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"x\"]\n[workspace.package]\nversion = \"9.9.9.lock\"\n"
            .to_owned(),
    );
    files.cargo_manifest_texts.insert(
        "tools/app/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n".to_owned(),
    );
    assert_eq!(refusal(&files), None);
    files.cargo_manifest_texts.insert(
        "tools/Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"a*\"]\n[workspace.package]\nversion = \"9.9.9.lock\"\n"
            .to_owned(),
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::VersionEndsInLock(..))
    ));
}

#[test]
fn an_unrelated_broken_manifest_under_the_root_does_not_block_generation() {
    let mut files = files();
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("app", "1.2.3"));
    files.cargo_manifest_texts.insert(
        "testdata/Cargo.toml".to_owned(),
        "[package\nname = ".to_owned(),
    );
    assert_eq!(refusal(&files), None);
}

#[test]
fn a_broken_workspace_manifest_a_member_inherits_from_fails_and_names_the_file() {
    let mut files = files();
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), "[workspace\nversion = ".to_owned());
    files.cargo_manifest_texts.insert(
        "app/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n".to_owned(),
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::InvalidToml { file, .. }) if file == "Cargo.toml"),
        "{error:?}"
    );
}
