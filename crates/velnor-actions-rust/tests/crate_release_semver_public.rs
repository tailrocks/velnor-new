// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    process::Command,
};
use velnor_actions_rust::release_select::{ReleaseRequest, ReleaseScope, select_release_set};

#[test]
fn cargo_prerelease_version_can_be_selected() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("src"))?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='probe'\nversion='1.2.3-18446744073709551616'\nedition='2021'\n",
    )?;
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(temp.path())
        .output()?;
    assert!(output.status.success());
    let metadata = String::from_utf8(output.stdout)?;
    let scope = ReleaseScope::Packages(vec!["probe".into()]);
    let changed = BTreeSet::new();
    let root = temp.path().canonicalize()?;
    let request = ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &changed,
        known_manifests: &BTreeSet::new(),
        supported_registries: &[],
    };
    assert_eq!(select_release_set(&request)?.names(), vec!["probe"]);
    Ok(())
}

#[test]
fn registry_graph_uses_cargo_prerelease_requirement_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_rust::{
        metadata::parse_metadata_json,
        release_graph::{RegistryPackage, RegistryState, publication_graph},
    };
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("src"))?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='probe'\nversion='1.0.0'\nedition='2021'\n[dependencies]\nexample = '>=1.2.3-18446744073709551616'\n",
    )?;
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(temp.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = String::from_utf8(output.stdout)?;
    let root = temp.path().canonicalize()?;
    let scope = ReleaseScope::Packages(vec!["probe".into()]);
    let changed = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &changed,
        known_manifests: &BTreeSet::new(),
        supported_registries: &[],
    };
    let selection = select_release_set(&request)?;
    let record = parse_metadata_json(&metadata, &root, "Cargo.toml", &BTreeSet::new())?;
    let requirement = semver::VersionReq::parse(">=1.2.3-18446744073709551616")?;
    for version in [
        "1.2.3-18446744073709551615",
        "1.2.3-18446744073709551616",
        "1.2.3-18446744073709551617+build",
        "1.2.4-alpha",
    ] {
        let state = RegistryState {
            published: BTreeMap::from([(
                RegistryPackage {
                    registry: "crates-io".into(),
                    package: "example".into(),
                },
                BTreeSet::from([version.into()]),
            )]),
        };
        assert_eq!(
            publication_graph(&selection, &record, &state, &[]).is_ok(),
            requirement.matches(&semver::Version::parse(version)?),
            "{version}"
        );
    }
    Ok(())
}
