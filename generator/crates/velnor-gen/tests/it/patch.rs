// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    AllTrigger, ChangeSet, DependencyKind, PackageDependency, PackageName, RepoView, RustPackage,
    RustSelection, SelectionReason,
};
use velnor_rust::{DetectError, Reason, detect, select, wanted_files};

use crate::facts::{TestResult, lib, path, repo};

fn uses(
    mut package: RustPackage,
    names: &[&str],
) -> Result<RustPackage, Box<dyn std::error::Error>> {
    for name in names {
        package.dependencies.push(PackageDependency {
            name: PackageName::new(*name)?,
            kind: DependencyKind::Normal,
            manifest: None,
        });
    }
    Ok(package)
}

fn two_roots(files: &[(&str, &str)]) -> Result<RepoView, Box<dyn std::error::Error>> {
    let app = uses(
        lib("app", "a/app/Cargo.toml")?,
        &["shared", "serde", "mid", "alias", "cfg", "inner", "other"],
    )?;
    repo(
        vec![
            ("a/Cargo.toml", vec![app]),
            ("b/Cargo.toml", vec![lib("shared", "b/shared/Cargo.toml")?]),
        ],
        files,
    )
}

#[test]
fn a_registry_dependency_that_the_repository_can_patch_is_an_edge_to_the_patch() -> TestResult {
    let view = two_roots(&[
        (
            "a/Cargo.toml",
            "[workspace]\n\n[patch.crates-io]\nmid = { path = \"../vendor/./mid\" }\nalias-patch = { package = \"alias\", path = \"../vendor/alias\" }\n",
        ),
        (
            ".cargo/config.toml",
            "[patch.crates-io]\ncfg = { path = \"vendor/cfg\" }\n",
        ),
        (
            "a/.cargo/config",
            "[patch.crates-io]\ninner = { path = \"inner\" }\n",
        ),
        (
            "b/.cargo/config.toml",
            "[patch.crates-io]\nother = { path = \"other\" }\n",
        ),
    ])?;
    let facts = detect(&view)?;
    let dependencies: BTreeSet<String> = facts.rust_roots[0]
        .members
        .iter()
        .flat_map(|member| &member.dependencies)
        .map(|d| format!("{} {}", d.name, d.manifest))
        .collect();
    assert_eq!(
        dependencies,
        BTreeSet::from(
            [
                "shared b/shared/Cargo.toml",
                "mid vendor/mid/Cargo.toml",
                "alias vendor/alias/Cargo.toml",
                "cfg vendor/cfg/Cargo.toml",
                "inner a/inner/Cargo.toml",
            ]
            .map(str::to_owned)
        )
    );
    Ok(())
}

#[test]
fn a_change_reaches_a_member_through_a_package_patched_in_cargo_configuration() -> TestResult {
    let view = repo(
        vec![
            (
                "a/Cargo.toml",
                vec![uses(lib("app", "a/app/Cargo.toml")?, &["mid"])?],
            ),
            ("b/Cargo.toml", vec![lib("base", "b/base/Cargo.toml")?]),
        ],
        &[(
            ".cargo/config.toml",
            "[patch.crates-io]\nmid = { path = \"vendor/mid\" }\n",
        )],
    )?;
    let changes = ChangeSet {
        paths: BTreeSet::from([path("b/base/src/lib.rs")?]),
        base_files: BTreeMap::new(),
    };
    let selections = select(&detect(&view)?, &changes);
    assert_eq!(
        selections.get(&path("a/Cargo.toml")?),
        Some(&RustSelection::All(AllTrigger::MostPackages(
            BTreeMap::from([(
                PackageName::new("app")?,
                SelectionReason::DependsOn(PackageName::new("mid")?),
            )])
        )))
    );
    Ok(())
}

#[test]
fn a_malformed_patch_table_is_an_error_naming_the_file_and_the_key() -> TestResult {
    let cases = [
        ("a/Cargo.toml", "patch = 1\n", Some("patch")),
        (
            "a/Cargo.toml",
            "[patch]\ncrates-io = 1\n",
            Some("patch.crates-io"),
        ),
        (
            "a/Cargo.toml",
            "[patch.crates-io]\nmid = \"1.0\"\n",
            Some("patch.crates-io.mid"),
        ),
        (
            "a/Cargo.toml",
            "[patch.crates-io]\nmid = { path = 1 }\n",
            Some("patch.crates-io.mid"),
        ),
        (
            "a/Cargo.toml",
            "[patch.crates-io]\nmid = { package = 1, path = \"m\" }\n",
            Some("patch.crates-io.mid"),
        ),
        (
            "a/Cargo.toml",
            "[patch.crates-io]\n\"mi\\td\" = { path = \"mid\" }\n",
            Some("patch.crates-io.mi\td"),
        ),
        (
            "a/Cargo.toml",
            "[patch.crates-io]\nmid = { path = \"m\\\\id\" }\n",
            Some("patch.crates-io.mid"),
        ),
        ("a/Cargo.toml", "[patch.crates-io\n", None),
        (
            ".cargo/config.toml",
            "[patch.crates-io]\nmid = { path = 1 }\n",
            Some("patch.crates-io.mid"),
        ),
        (".cargo/config.toml", "[patch.crates-io\n", None),
    ];
    for (file, text, key) in cases {
        let result = detect(&two_roots(&[(file, text)])?);
        let reason = match &result {
            Err(DetectError::Invalid { path, reason }) if path.as_str() == file => reason,
            other => return Err(format!("{file} {text:?}: {other:?}").into()),
        };
        match (key, reason) {
            (Some(key), Reason::InvalidPatch(found)) => assert_eq!(found, key, "{text:?}"),
            (None, Reason::Syntax(_)) => {}
            (_, other) => return Err(format!("{file} {text:?}: {other:?}").into()),
        }
    }
    for text in [
        "[patch.crates-io]\nmid = { git = \"https://example.com/mid\" }\n",
        "[patch.crates-io]\nmid = { path = \"../../outside\" }\n",
    ] {
        detect(&two_roots(&[("a/Cargo.toml", text)])?)?;
    }
    Ok(())
}

#[test]
fn the_generator_reads_every_tracked_cargo_configuration() -> TestResult {
    let tracked: BTreeSet<_> = [
        ".cargo/config.toml",
        "a/.cargo/config",
        "a/cargo/config",
        "a/.cargo/config.json",
        "Cargo.toml",
    ]
    .into_iter()
    .map(path)
    .collect::<Result<_, _>>()?;
    let wanted: Vec<String> = wanted_files(&tracked)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(wanted, [".cargo/config.toml", "a/.cargo/config"]);
    Ok(())
}
