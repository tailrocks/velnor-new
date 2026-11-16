// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles};

use crate::fixture::{PLZ, TestResult, files, plz_edited, refusal, with_metadata};

fn manifest(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")
}

fn set_version(files: &mut ReleaseFiles, manifest: &str, version: &str) -> TestResult {
    let metadata = files.rust_metadata.as_mut().ok_or("no metadata")?;
    for member in metadata.values_mut().flat_map(|known| &mut known.members) {
        if member.manifest.as_str() == manifest {
            member.version = Some(version.to_owned());
        }
    }
    Ok(())
}

fn lock_refusal(files: &ReleaseFiles, manifest: &str) -> bool {
    matches!(refusal(files), Some(ReleaseError::VersionEndsInLock(path, _)) if path == manifest)
}

fn member_root(files: &mut ReleaseFiles) {
    files.release_toml = Some("roots = [\"member\"]\n".to_owned());
    files.release_plz_toml.clear();
    files
        .release_plz_toml
        .insert("member".to_owned(), PLZ.to_owned());
}

#[test]
fn a_release_false_package_with_a_lock_version_does_not_block_generation() -> TestResult {
    let mut files = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"skip\"\nrelease = false",
    )?;
    with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("skip", "skip/Cargo.toml")],
        )],
    );
    set_version(&mut files, "skip/Cargo.toml", "1.2.3-alpha.lock")?;
    assert_eq!(refusal(&files), None);
    set_version(&mut files, "Cargo.toml", "1.2.3-alpha.lock")?;
    assert!(lock_refusal(&files, "Cargo.toml"));
    Ok(())
}

#[test]
fn a_workspace_inherited_lock_version_is_refused_from_the_metadata_version() -> TestResult {
    let mut files = files();
    member_root(&mut files);
    with_metadata(
        &mut files,
        &[("Cargo.toml", &[("app", "member/Cargo.toml")])],
    );
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n".to_owned(),
    );
    set_version(&mut files, "member/Cargo.toml", "1.2.3-alpha.lock")?;
    assert!(lock_refusal(&files, "member/Cargo.toml"));
    set_version(&mut files, "member/Cargo.toml", "1.2.3")?;
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn a_nested_non_member_workspace_does_not_supply_the_version() -> TestResult {
    let mut files = files();
    with_metadata(
        &mut files,
        &[("Cargo.toml", &[("app", "tools/app/Cargo.toml")])],
    );
    files.cargo_manifest_texts.insert(
        "tools/Cargo.toml".to_owned(),
        "[workspace]\n[workspace.package]\nversion = \"9.9.9-alpha.lock\"\n".to_owned(),
    );
    files.cargo_manifest_texts.insert(
        "tools/app/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n".to_owned(),
    );
    set_version(&mut files, "tools/app/Cargo.toml", "1.2.3")?;
    assert_eq!(refusal(&files), None);
    set_version(&mut files, "tools/app/Cargo.toml", "9.9.9-alpha.lock")?;
    assert!(lock_refusal(&files, "tools/app/Cargo.toml"));
    Ok(())
}

#[test]
fn an_explicit_package_workspace_supplies_the_version_through_metadata() -> TestResult {
    let mut files = files();
    member_root(&mut files);
    with_metadata(
        &mut files,
        &[("owner/Cargo.toml", &[("app", "member/Cargo.toml")])],
    );
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nworkspace = \"../owner\"\nversion.workspace = true\n"
            .to_owned(),
    );
    set_version(&mut files, "member/Cargo.toml", "1.2.3-alpha.lock")?;
    assert!(lock_refusal(&files, "member/Cargo.toml"));
    Ok(())
}

#[test]
fn without_metadata_only_a_literal_version_is_checked() {
    let mut files = files();
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("app", "1.2.3-alpha.lock"));
    assert!(lock_refusal(&files, "Cargo.toml"));
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n[workspace.package]\nversion = \"1.2.3-alpha.lock\"\n"
            .to_owned(),
    );
    assert_eq!(refusal(&files), None);
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
fn a_member_recorded_without_a_version_falls_back_to_its_literal_version() -> TestResult {
    let mut files = files();
    member_root(&mut files);
    with_metadata(
        &mut files,
        &[("Cargo.toml", &[("app", "member/Cargo.toml")])],
    );
    for member in files
        .rust_metadata
        .as_mut()
        .ok_or("no metadata")?
        .values_mut()
        .flat_map(|known| &mut known.members)
    {
        member.version = None;
    }
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        manifest("app", "1.2.3-alpha.lock"),
    );
    assert!(lock_refusal(&files, "member/Cargo.toml"));
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion.workspace = true\n[workspace.package]\nversion = \"1.2.3-alpha.lock\"\n"
            .to_owned(),
    );
    assert_eq!(refusal(&files), None);
    Ok(())
}
