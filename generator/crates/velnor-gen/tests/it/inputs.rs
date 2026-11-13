// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoPath, RepoView, RustMetadata, RustPackage,
    Target, TargetKind, TargetName,
};
use velnor_rust::{MetadataError, metadata_roots, reduce_metadata, wanted_files};

type TestResult = Result<(), Box<dyn Error>>;

fn path(text: &str) -> Result<RepoPath, Box<dyn Error>> {
    Ok(RepoPath::new(text)?)
}

fn dependency(
    name: &str,
    kind: DependencyKind,
    manifest: Option<&str>,
) -> Result<PackageDependency, Box<dyn Error>> {
    Ok(PackageDependency {
        name: PackageName::new(name)?,
        kind,
        manifest: manifest.map(path).transpose()?,
    })
}

fn target(name: &str, kinds: Vec<TargetKind>) -> Result<Target, Box<dyn Error>> {
    Ok(Target {
        name: TargetName::new(name)?,
        kinds,
    })
}

#[test]
fn cargo_metadata_reduces_to_members_dependency_edges_and_targets() -> TestResult {
    let json = br#"{
      "workspace_root": "/repo",
      "workspace_members": ["path+file:///repo/app#0.0.0"],
      "packages": [{
        "name": "app",
        "version": "0.0.0",
        "manifest_path": "/repo/app/Cargo.toml",
        "dependencies": [
          {"name": "lib", "kind": null, "path": "/repo/lib", "source": null},
          {"name": "serde", "kind": null, "source": "registry+https://github.com/rust-lang/crates.io-index"},
          {"name": "helper", "kind": "dev", "path": "/repo/tools/helper"},
          {"name": "root", "kind": "build", "path": "/repo"}
        ],
        "targets": [
          {"name": "app", "kind": ["bin"], "src_path": "/repo/app/src/main.rs"},
          {"name": "derive", "kind": ["proc-macro"]},
          {"name": "build-script-build", "kind": ["custom-build"]},
          {"name": "smoke", "kind": ["test"]}
        ]
      }]
    }"#;
    let expected = RustMetadata {
        members: vec![RustPackage {
            name: PackageName::new("app")?,
            manifest: path("app/Cargo.toml")?,
            dependencies: vec![
                dependency("lib", DependencyKind::Normal, Some("lib/Cargo.toml"))?,
                dependency("serde", DependencyKind::Normal, None)?,
                dependency(
                    "helper",
                    DependencyKind::Dev,
                    Some("tools/helper/Cargo.toml"),
                )?,
                dependency("root", DependencyKind::Build, Some("Cargo.toml"))?,
            ],
            targets: vec![
                target("app", vec![TargetKind::Bin])?,
                target("derive", vec![TargetKind::Lib])?,
                target("build-script-build", vec![TargetKind::BuildScript])?,
                target("smoke", vec![TargetKind::Test])?,
            ],
        }],
    };
    assert_eq!(
        reduce_metadata(Path::new("/repo"), json)?,
        (path("Cargo.toml")?, expected)
    );
    Ok(())
}

#[test]
fn a_path_outside_the_repository_is_an_error() {
    let package = |manifest: &str, dependency: &str| {
        format!(
            r#"{{"name": "a", "manifest_path": "{manifest}", "targets": [],
                "dependencies": [{{"name": "b", "kind": null, "path": "{dependency}"}}]}}"#
        )
    };
    let cases = [
        ("/repo", package("/repo/a/Cargo.toml", "/elsewhere/b")),
        ("/repo", package("/repo/a/Cargo.toml", "/repo/../b")),
        ("/repo", package("/repo/a/Cargo.toml", "/repository/b")),
        ("/repo", package("/other/a/Cargo.toml", "/repo/b")),
        ("/other", package("/repo/a/Cargo.toml", "/repo/b")),
    ];
    for (workspace_root, package) in cases {
        let json = format!(r#"{{"workspace_root": "{workspace_root}", "packages": [{package}]}}"#);
        let result = reduce_metadata(Path::new("/repo"), json.as_bytes());
        assert!(
            matches!(result, Err(MetadataError::OutsideRepository(_))),
            "{json} gave {result:?}"
        );
    }
}

#[test]
fn io_reads_the_tracked_config_and_runs_metadata_on_manifests_outside_exclude_shallowest_first()
-> TestResult {
    let mut view = RepoView::default();
    for tracked in [
        "Cargo.toml",
        "b/Cargo.toml",
        "a/c/Cargo.toml",
        "a/Cargo.toml",
        "fixtures/broken/Cargo.toml",
        "README.md",
        "mise.toml",
        ".velnor/rust-ci.toml",
    ] {
        view.tracked_paths.insert(path(tracked)?);
    }
    assert_eq!(
        wanted_files(&view.tracked_paths),
        vec![path(".velnor/rust-ci.toml")?, path("mise.toml")?]
    );
    view.files.insert(
        path(".velnor/rust-ci.toml")?,
        b"exclude = [\"fixtures\"]\n".to_vec(),
    );
    assert_eq!(
        metadata_roots(&view)?,
        vec![
            path("Cargo.toml")?,
            path("a/Cargo.toml")?,
            path("b/Cargo.toml")?,
            path("a/c/Cargo.toml")?,
        ]
    );
    Ok(())
}
