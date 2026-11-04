// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn dev_path_wildcard_presence_matches_cargo_package() {
    use velnor_actions_rust::release_graph::{
        RegistryPackage, plan_release, registry_requirements,
    };
    use velnor_actions_rust::release_select::select_release_set;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("root");
    write(&root, "src/lib.rs", "pub fn f() {}\n");
    write(&root, "dep/src/lib.rs", "pub fn g() {}\n");
    write(
        &root,
        "dep/Cargo.toml",
        "[package]\nname='unique_missing_probe_487'\nversion='1.0.0'\nedition='2021'\n",
    );
    let scope = ReleaseScope::Packages(vec!["workspace_probe_487".to_owned()]);
    let empty = BTreeSet::new();
    let known = BTreeSet::from(["dep/Cargo.toml".to_owned()]);
    for (version, expected) in [("", true), (",version='*'", false)] {
        write(
            &root,
            "Cargo.toml",
            &format!(
                "[package]\nname='workspace_probe_487'\nversion='1.0.0'\nedition='2021'\n[workspace]\nexclude=['dep']\n[dev-dependencies]\nunique_missing_probe_487={{path='dep'{version}}}\n"
            ),
        );
        let metadata =
            wildcard_fixture_metadata(&root, version, expected).expect("fixture metadata");
        let request = ReleaseRequest {
            metadata_json: &metadata,
            repo_root: &root,
            manifest_hint: "Cargo.toml",
            scope: &scope,
            enabled: true,
            affected: &empty,
            known_manifests: &known,
            supported_registries: &[],
        };
        let selection = select_release_set(&request).expect("selection");
        let requirements = registry_requirements(&selection).expect("requirements");
        assert_eq!(
            requirements.contains(&RegistryPackage {
                registry: "crates-io".into(),
                package: "unique_missing_probe_487".into(),
            }),
            !expected
        );
        let result = plan_release(&request, &RegistryState::default());
        assert_eq!(
            result.is_ok(),
            expected,
            "planner differs from Cargo for {version}: {result:?}"
        );
        if !expected {
            let mut available = RegistryState::default();
            available.published.insert(
                RegistryPackage {
                    registry: "crates-io".into(),
                    package: "unique_missing_probe_487".into(),
                },
                BTreeSet::from(["1.0.0".into()]),
            );
            assert!(
                plan_release(&request, &available).is_ok(),
                "declared wildcard can use registry version"
            );
        }
    }
}

fn wildcard_fixture_metadata(
    root: &Path,
    version: &str,
    expected: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root)
        .env("CARGO_HOME", root.join("cargo-home"))
        .output()?;
    assert!(metadata.status.success());
    let metadata = String::from_utf8(metadata.stdout)?;
    let cargo = Command::new(env!("CARGO"))
        .args(["package", "--no-verify", "--offline"])
        .current_dir(root)
        .env("CARGO_HOME", root.join("cargo-home"))
        .output()?;
    assert_eq!(
        cargo.status.success(),
        expected,
        "Cargo case {version}: {}",
        String::from_utf8_lossy(&cargo.stderr)
    );
    Ok(metadata)
}

#[test]
fn renamed_inherited_target_dev_wildcard_matches_cargo_package() {
    use velnor_actions_rust::release_graph::plan_release;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("root");
    write(&root, "app/src/lib.rs", "pub fn f() {}\n");
    write(&root, "helper/src/lib.rs", "pub fn g() {}\n");
    write(
        &root,
        "helper/Cargo.toml",
        "[package]\nname='unique_missing_inherited_487'\nversion='1.0.0'\nedition='2021'\n",
    );
    write(
        &root,
        "app/Cargo.toml",
        "[package]\nname='inherited_probe_487'\nversion='1.0.0'\nedition='2021'\n[target.'cfg( unix )'.dev-dependencies]\nrenamed.workspace=true\n",
    );
    let scope = ReleaseScope::Packages(vec!["inherited_probe_487".into()]);
    let empty = BTreeSet::new();
    let known = BTreeSet::from(["helper/Cargo.toml".to_owned()]);
    for (version, expected) in [("", true), (",version='*'", false)] {
        write(
            &root,
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers=['app']\nexclude=['helper']\nresolver='2'\n[workspace.dependencies]\nrenamed={{package='unique_missing_inherited_487',path='helper'{version}}}\n"
            ),
        );
        let metadata = Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--offline",
            ])
            .current_dir(&root)
            .env("CARGO_HOME", root.join("cargo-home"))
            .output()
            .expect("metadata");
        assert!(
            metadata.status.success(),
            "{}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        let metadata = String::from_utf8(metadata.stdout).expect("utf8");
        let request = ReleaseRequest {
            metadata_json: &metadata,
            repo_root: &root,
            manifest_hint: "app/Cargo.toml",
            scope: &scope,
            enabled: true,
            affected: &empty,
            known_manifests: &known,
            supported_registries: &[],
        };
        let cargo = Command::new(env!("CARGO"))
            .args([
                "package",
                "--manifest-path",
                "app/Cargo.toml",
                "--no-verify",
                "--offline",
            ])
            .current_dir(&root)
            .env("CARGO_HOME", root.join("cargo-home"))
            .output()
            .expect("cargo package");
        assert_eq!(
            cargo.status.success(),
            expected,
            "Cargo case {version}: {}",
            String::from_utf8_lossy(&cargo.stderr)
        );
        let result = plan_release(&request, &RegistryState::default());
        assert_eq!(
            result.is_ok(),
            expected,
            "planner differs from Cargo for {version}: {result:?}"
        );
    }
}

#[test]
fn dev_git_wildcard_presence_matches_cargo_package() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("root");
    write(&root, "src/lib.rs", "pub fn f() {}\n");
    write(&root, "dep/src/lib.rs", "pub fn g() {}\n");
    write(
        &root,
        "dep/Cargo.toml",
        "[package]\nname='unique_git_missing_487'\nversion='1.0.0'\nedition='2021'\n",
    );
    let url = git_fixture_url(&root);
    let scope = ReleaseScope::Packages(vec!["workspace_git_probe_487".into()]);
    let empty = BTreeSet::new();
    for (version, expected) in [("", true), (",version='*'", false)] {
        write(
            &root,
            "Cargo.toml",
            &format!(
                "[package]\nname='workspace_git_probe_487'\nversion='1.0.0'\nedition='2021'\n[workspace]\nexclude=['dep']\n[dev-dependencies]\nunique_git_missing_487={{git='{url}'{version}}}\n"
            ),
        );
        let metadata = Command::new(env!("CARGO"))
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(&root)
            .env("CARGO_HOME", root.join("cargo-home"))
            .output()
            .expect("metadata");
        assert!(
            metadata.status.success(),
            "{}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        let metadata = String::from_utf8(metadata.stdout).expect("utf8");
        let request = ReleaseRequest {
            metadata_json: &metadata,
            repo_root: &root,
            manifest_hint: "Cargo.toml",
            scope: &scope,
            enabled: true,
            affected: &empty,
            known_manifests: &empty,
            supported_registries: &[],
        };
        let cargo = Command::new(env!("CARGO"))
            .args(["package", "--no-verify", "--offline"])
            .current_dir(&root)
            .env("CARGO_HOME", root.join("cargo-home"))
            .output()
            .expect("cargo package");
        assert_eq!(
            cargo.status.success(),
            expected,
            "Cargo case {version}: {}",
            String::from_utf8_lossy(&cargo.stderr)
        );
        let result = plan_release(&request, &RegistryState::default());
        assert_eq!(
            result.is_ok(),
            expected,
            "planner differs from Cargo for {version}: {result:?}"
        );
        if !expected {
            let mut available = RegistryState::default();
            available.published.insert(
                RegistryPackage {
                    registry: "crates-io".into(),
                    package: "unique_git_missing_487".into(),
                },
                BTreeSet::from(["1.0.0".into()]),
            );
            assert!(plan_release(&request, &available).is_ok());
        }
    }
}

fn git_fixture_url(root: &Path) -> String {
    let dep = root.join("dep");
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "add",
            ".",
        ],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&dep)
                .status()
                .is_ok_and(|status| status.success())
        );
    }
    let path = dep.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(unc) = path.strip_prefix("//?/UNC/") {
            format!("file://{unc}")
        } else {
            format!("file:///{}", path.strip_prefix("//?/").unwrap_or(&path))
        }
    } else {
        format!("file://{path}")
    }
}

#[test]
fn equivalent_target_tables_keep_consistent_dependency_versions() {
    use velnor_actions_rust::release_graph::plan_release;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("root");
    write(&root, "src/lib.rs", "pub fn f() {}\n");
    write(&root, "dep/src/lib.rs", "pub fn f() {}\n");
    write(
        &root,
        "dep/Cargo.toml",
        "[package]\nname='cfg_probe_dep'\nversion='1.0.0'\nedition='2021'\n",
    );
    write(
        &root,
        "Cargo.toml",
        "[package]\nname='cfg_probe_app'\nversion='1.0.0'\nedition='2021'\n[workspace]\nexclude=['dep']\n[target.'cfg(unix)'.dev-dependencies]\ncfg_probe_dep={path='dep'}\n[target.'cfg( unix )'.dev-dependencies]\ncfg_probe_dep={path='dep'}\n",
    );
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(&root)
        .output()
        .expect("metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cargo = Command::new(env!("CARGO"))
        .args(["package", "--offline", "--no-verify"])
        .current_dir(&root)
        .output()
        .expect("package");
    assert!(
        cargo.status.success(),
        "{}",
        String::from_utf8_lossy(&cargo.stderr)
    );
    let metadata = String::from_utf8(output.stdout).expect("utf8");
    let known = BTreeSet::from(["dep/Cargo.toml".to_owned()]);
    let scope = ReleaseScope::Packages(vec!["cfg_probe_app".into()]);
    let empty = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &known,
        supported_registries: &[],
    };
    let result = plan_release(&request, &RegistryState::default());
    assert!(
        result.is_ok(),
        "Cargo packaged consistent declarations but planner rejected: {result:?}"
    );
}
