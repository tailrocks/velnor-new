// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoPath, RepoView, RequiredFeature,
    RustMetadata, RustPackage, Target, TargetKind, TargetName,
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
        default_features: true,
        name: PackageName::new(name)?,
        kind,
        platform: None,
        optional: false,
        manifest: manifest.map(path).transpose()?,
        features: Vec::new(),
        rename: None,
    })
}

fn target(
    name: &str,
    kinds: Vec<TargetKind>,
    tests: bool,
    source: Option<&str>,
) -> Result<Target, Box<dyn Error>> {
    let bench = kinds.iter().any(|kind| {
        matches!(
            kind,
            TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin | TargetKind::Bench
        )
    });
    Ok(Target {
        name: TargetName::new(name)?,
        kinds,
        tests,
        doctest: true,
        source: source.map(path).transpose()?,
        requires_features: name == "derive",
        required_features: if name == "derive" {
            vec![RequiredFeature {
                name: "macros".to_owned(),
                enabled_by: vec!["default".to_owned(), "full".to_owned()],
            }]
        } else {
            Vec::new()
        },
        doc: true,
        bench,
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
          {"name": "app", "kind": ["bin"], "src_path": "/repo/app/src/main.rs", "test": true},
          {"name": "derive", "kind": ["proc-macro"], "test": false, "required-features": ["macros"]},
          {"name": "build-script-build", "kind": ["custom-build"], "test": false},
          {"name": "smoke", "kind": ["test"], "src_path": "/elsewhere/smoke.rs", "test": true}
        ],
        "features": {"default": ["full"], "full": ["macros"], "macros": [], "unrelated": ["serde/derive"]}
      }]
    }"#;
    let expected = RustMetadata {
        members: vec![RustPackage {
            features: std::collections::BTreeMap::new(),
            name: PackageName::new("app")?,
            version: Some("0.0.0".to_owned()),
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
                target("app", vec![TargetKind::Bin], true, Some("app/src/main.rs"))?,
                target("derive", vec![TargetKind::ProcMacro], false, None)?,
                target(
                    "build-script-build",
                    vec![TargetKind::BuildScript],
                    false,
                    None,
                )?,
                target("smoke", vec![TargetKind::Test], true, None)?,
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
            r#"{{"name": "a", "version": "0.0.0", "manifest_path": "{manifest}", "targets": [],
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

#[test]
fn cargo_metadata_carries_the_flags_and_feature_table_selection_needs() -> TestResult {
    let scratch = velnor_test_support::ScratchDir::new(
        std::env::temp_dir().join(format!("velnor-gen-target-flags-{}", std::process::id())),
    );
    let manifest = scratch.path().join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\n[lib]\nbench = false\n[[bin]]\nname = \"tool\"\npath = \"src/tool.rs\"\nbench = false\n",
    )?;
    let root = scratch.path().to_string_lossy();
    let json = format!(
        r#"{{
      "workspace_root": "{root}",
      "packages": [{{
        "name": "app",
        "version": "0.0.0",
        "manifest_path": "{root}/Cargo.toml",
        "features": {{"default": ["dep:helper"], "extra": []}},
        "dependencies": [
          {{"name": "helper", "kind": null, "optional": true, "uses_default_features": false, "target": "cfg(unix)", "path": "{root}/helper"}}
        ],
        "targets": [
          {{"name": "app", "kind": ["lib"], "test": true}},
          {{"name": "tool", "kind": ["bin"], "test": true, "doc": false, "required-features": ["extra"]}},
          {{"name": "speed", "kind": ["bench"], "test": false}}
        ]
      }}]
    }}"#
    );
    let (_, reduced) = reduce_metadata(scratch.path(), json.as_bytes())?;
    let package = &reduced.members[0];
    let flags: Vec<(&str, bool, bool, Vec<String>)> = package
        .targets
        .iter()
        .map(|t| {
            let names = t.required_features.iter().map(|f| f.name.clone());
            (t.name.as_str(), t.doc, t.bench, names.collect())
        })
        .collect();
    assert_eq!(
        flags,
        [
            ("app", true, false, vec![]),
            ("tool", false, false, vec!["extra".to_owned()]),
            ("speed", true, true, vec![]),
        ]
    );
    assert_eq!(
        package.features.get("default"),
        Some(&vec!["dep:helper".to_owned()])
    );
    assert!(!package.dependencies[0].default_features);
    assert_eq!(
        package.dependencies[0].platform.as_deref(),
        Some("cfg(unix)")
    );
    Ok(())
}
