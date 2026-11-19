// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A workspace crate that a shipped binary compiles in changes the binary, so
//! release-plz.toml must list it (#1628). The real repository's binaries
//! reach only listed crates.

use std::collections::BTreeMap;

use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoPath, RustMetadata, RustPackage,
};
use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan, ShippedError};

use crate::fixture::{PLZ, TestResult, files};

const BINARY: &str = "[[binaries]]\npackage = \"app\"\n";

fn name(text: &str) -> Result<PackageName, Box<dyn std::error::Error>> {
    Ok(PackageName::new(text.to_owned())?)
}

fn package(
    text: &str,
    manifest: &str,
    deps: &[(&str, DependencyKind)],
) -> Result<RustPackage, Box<dyn std::error::Error>> {
    let dependencies = deps
        .iter()
        .map(|(dep, kind)| {
            Ok(PackageDependency {
                name: name(dep)?,
                kind: *kind,
                platform: None,
                optional: false,
                manifest: None,
                features: Vec::new(),
                rename: None,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(RustPackage {
        name: name(text)?,
        version: Some("0.1.0".to_owned()),
        manifest: RepoPath::new(manifest)?,
        dependencies,
        targets: Vec::new(),
    })
}

/// The binary `app` normally depends on `lib`, which depends on `core` only
/// for its build script; `dev_only` is a dev-dependency of `app`.
fn app_metadata() -> Result<BTreeMap<RepoPath, RustMetadata>, Box<dyn std::error::Error>> {
    let members = vec![
        package(
            "app",
            "app/Cargo.toml",
            &[
                ("lib", DependencyKind::Normal),
                ("dev_only", DependencyKind::Dev),
            ],
        )?,
        package("lib", "lib/Cargo.toml", &[("core", DependencyKind::Build)])?,
        package("core", "core/Cargo.toml", &[])?,
        package("dev_only", "dev_only/Cargo.toml", &[])?,
    ];
    Ok([(RepoPath::new("Cargo.toml")?, RustMetadata { members })].into())
}

fn app_files(release_plz: &str) -> Result<ReleaseFiles, Box<dyn std::error::Error>> {
    let mut files = files();
    files.release_toml = Some(BINARY.to_owned());
    files
        .release_plz_toml
        .insert(".".to_owned(), release_plz.to_owned());
    for manifest in [
        "Cargo.toml",
        "app/Cargo.toml",
        "lib/Cargo.toml",
        "core/Cargo.toml",
        "dev_only/Cargo.toml",
    ] {
        files.cargo_manifest_texts.insert(
            manifest.to_owned(),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n".to_owned(),
        );
    }
    files.rust_metadata = Some(app_metadata()?);
    Ok(files)
}

#[test]
fn a_crate_a_shipped_binary_compiles_in_must_be_listed() -> TestResult {
    let error = ReleasePlan::read(&app_files(PLZ)?).err();
    assert!(
        matches!(
            &error,
            Some(ReleaseError::Shipped(ShippedError::Unlisted { binary, package }))
                if binary == "app" && (package == "lib" || package == "core")
        ),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_listed_closure_is_accepted_and_dev_only_crates_are_not_required() -> TestResult {
    let text = [PLZ, "\n[[package]]\nname = \"lib\"\nrelease = false\n\n[[package]]\nname = \"core\"\nrelease = false\n"].concat();
    let error = ReleasePlan::read(&app_files(&text)?).err();
    assert!(error.is_none(), "{error:?}");
    Ok(())
}

#[test]
fn the_real_binaries_reach_only_listed_crates() -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| std::fs::read_to_string(root.join(path));
    let mut files = files();
    files.release_toml = Some(read(".velnor/release.toml")?);
    files
        .release_plz_toml
        .insert(".".to_owned(), read("release-plz.toml")?);
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), read("Cargo.toml")?);
    files.rust_metadata = Some(recorded_metadata(&read(".velnor/rust-metadata.json")?)?);
    let error = ReleasePlan::read(&files).err();
    assert!(error.is_none(), "{error:?}");
    Ok(())
}

/// The recorded `.velnor/rust-metadata.json`, reduced to what the closure reads.
fn recorded_metadata(
    text: &str,
) -> Result<BTreeMap<RepoPath, RustMetadata>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let roots = value["roots"].as_object().ok_or("no roots")?;
    let mut metadata = BTreeMap::new();
    for (workspace, root) in roots {
        let mut members = Vec::new();
        for member in root["members"].as_array().ok_or("no members")? {
            let mut dependencies = Vec::new();
            for dep in member["dependencies"].as_array().ok_or("no dependencies")? {
                dependencies.push(PackageDependency {
                    name: name(dep["name"].as_str().ok_or("no name")?)?,
                    kind: match dep["kind"].as_str() {
                        Some("normal") => DependencyKind::Normal,
                        Some("build") => DependencyKind::Build,
                        Some("dev") => DependencyKind::Dev,
                        other => return Err(format!("kind {other:?}").into()),
                    },
                    platform: dep["platform"].as_str().map(str::to_owned),
                    optional: false,
                    manifest: None,
                    features: Vec::new(),
                    rename: None,
                });
            }
            members.push(RustPackage {
                name: name(member["name"].as_str().ok_or("no name")?)?,
                version: member["version"].as_str().map(str::to_owned),
                manifest: RepoPath::new(member["manifest"].as_str().ok_or("no manifest")?)?,
                dependencies,
                targets: Vec::new(),
            });
        }
        metadata.insert(RepoPath::new(workspace)?, RustMetadata { members });
    }
    Ok(metadata)
}
