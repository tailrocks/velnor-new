// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn git_fallback_and_unversioned_path_follow_cargo_publish_rules() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(&root, "src/lib.rs", "pub fn app() {}\n");
    write(
        &root,
        "dep/Cargo.toml",
        "[package]\nname=\"probe_local\"\nversion=\"1.0.0\"\nedition=\"2021\"\n",
    );
    write(&root, "dep/src/lib.rs", "pub fn dep() {}\n");
    let scope = ReleaseScope::Packages(vec!["probe_app".to_owned()]);
    let empty = BTreeSet::new();
    let known = BTreeSet::from(["dep/Cargo.toml".to_owned()]);
    let mut state = RegistryState::default();
    state.published.insert(
        RegistryPackage {
            registry: "crates-io".to_owned(),
            package: "probe_git".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    for (section, spec, succeeds) in [
        (
            "dependencies",
            "probe_git={git=\"https://example.invalid/probe.git\",version=\"1\"}",
            true,
        ),
        (
            "dependencies",
            "probe_git={git=\"https://example.invalid/probe.git\"}",
            false,
        ),
        ("dependencies", "probe_local={path=\"dep\"}", false),
        ("dev-dependencies", "probe_local={path=\"dep\"}", true),
    ] {
        write(
            &root,
            "Cargo.toml",
            &format!(
                "[package]\nname=\"probe_app\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[{section}]\n{spec}\n"
            ),
        );
        let output = Command::new(env!("CARGO"))
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(&root)
            .env("CARGO_HOME", root.join("cargo-home"))
            .output()
            .expect("cargo metadata");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json = String::from_utf8(output.stdout).expect("utf8");
        let request = ReleaseRequest {
            metadata_json: &json,
            repo_root: &root,
            manifest_hint: "Cargo.toml",
            scope: &scope,
            enabled: true,
            affected: &empty,
            known_manifests: &known,
            supported_registries: &[],
        };
        let result = plan_release(&request, &state);
        assert_eq!(result.is_ok(), succeeds, "{section}: {spec}: {result:?}");
    }
}

fn assert_unresolved(result: &Result<velnor_actions_rust::ReleasePlan, ReleaseError>) {
    assert!(
        matches!(
            result,
            Err(ReleaseError::Metadata(
                velnor_actions_rust::metadata::MetadataError::UnresolvedPathEdge { .. }
            ))
        ),
        "{result:?}"
    );
}

#[test]
fn source_escape_cannot_be_authorized_as_a_known_manifest() {
    use velnor_actions_rust::release_graph::plan_release;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("source");
    fs::create_dir(&root).expect("source");
    let root = root.canonicalize().expect("canonical source");
    write(
        &root,
        "Cargo.toml",
        "[package]\nname=\"probe_app\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dependencies]\nprobe_escape={path=\"../outside\",version=\"1\"}\n",
    );
    write(&root, "src/lib.rs", "pub fn app() {}\n");
    write(
        temp.path(),
        "outside/Cargo.toml",
        "[package]\nname=\"probe_escape\"\nversion=\"1.0.0\"\nedition=\"2021\"\n",
    );
    write(temp.path(), "outside/src/lib.rs", "pub fn outside() {}\n");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .env("CARGO_HOME", root.join("cargo-home"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("utf8");
    let scope = ReleaseScope::Packages(vec!["probe_app".to_owned()]);
    let empty = BTreeSet::new();
    let known = BTreeSet::from(["../outside/Cargo.toml".to_owned()]);
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &known,
        supported_registries: &[],
    };
    let mut state = RegistryState::default();
    state.published.insert(
        velnor_actions_rust::RegistryPackage {
            registry: "crates-io".to_owned(),
            package: "probe_escape".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    assert_unresolved(&plan_release(&request, &state));
    let mut altered: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    altered["packages"][0]["dependencies"][0]["path"] =
        root.join("../outside").to_string_lossy().to_string().into();
    let forged = serde_json::to_string(&altered).expect("JSON");
    let forged_request = ReleaseRequest {
        metadata_json: &forged,
        ..request.clone()
    };
    assert_unresolved(&plan_release(&forged_request, &state));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("outside"), root.join("link"))
            .expect("symlink");
        altered["packages"][0]["dependencies"][0]["path"] =
            root.join("link").to_string_lossy().to_string().into();
        let forged = serde_json::to_string(&altered).expect("JSON");
        let known = BTreeSet::from(["link/Cargo.toml".to_owned()]);
        let forged_request = ReleaseRequest {
            metadata_json: &forged,
            known_manifests: &known,
            ..request
        };
        assert_unresolved(&plan_release(&forged_request, &state));
    }
}

#[test]
fn selected_dev_dependency_is_published_before_dependent_when_unavailable() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers=[\"a\",\"b\"]\nresolver=\"2\"\n",
    );
    write(
        &root,
        "a/Cargo.toml",
        "[package]\nname=\"order_a\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dev-dependencies]\norder_b={path=\"../b\",version=\"=1.0.0\"}\n",
    );
    write(
        &root,
        "b/Cargo.toml",
        "[package]\nname=\"order_b\"\nversion=\"1.0.0\"\nedition=\"2021\"\n",
    );
    write(&root, "a/src/lib.rs", "pub fn a() {}\n");
    write(&root, "b/src/lib.rs", "pub fn b() {}\n");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .env("CARGO_HOME", root.join("cargo-home"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("utf8");
    let scope = ReleaseScope::Packages(vec!["order_a".to_owned(), "order_b".to_owned()]);
    let empty = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &empty,
        supported_registries: &[],
    };
    let plan = plan_release(&request, &RegistryState::default()).expect("dependency order exists");
    assert_eq!(
        plan.graph.order,
        ["order_b", "order_a"],
        "Cargo packaging needs the selected dev dependency published first when absent"
    );
    let mut registry = RegistryState::default();
    registry.published.insert(
        RegistryPackage {
            registry: "crates-io".into(),
            package: "order_b".into(),
        },
        BTreeSet::from(["1.0.0".into()]),
    );
    let plan = plan_release(&request, &registry).expect("existing dependency permits release");
    assert_eq!(plan.graph.order, ["order_a", "order_b"]);
}

#[test]
fn disabled_release_plan_does_not_read_metadata() {
    use velnor_actions_rust::release_graph::plan_release;
    use velnor_actions_rust::release_select::ResolvedScope;
    let scope = ReleaseScope::Packages(vec!["probe_app".into()]);
    let empty = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: "not JSON",
        repo_root: Path::new("/missing"),
        manifest_hint: "../invalid",
        scope: &scope,
        enabled: false,
        affected: &empty,
        known_manifests: &empty,
        supported_registries: &[],
    };
    let plan = plan_release(&request, &RegistryState::default()).expect("disabled plan");
    assert_eq!(plan.selection.scope, ResolvedScope::Disabled);
    assert!(plan.selection.is_empty());
    assert!(plan.graph.is_empty());
    let enabled = ReleaseRequest {
        enabled: true,
        ..request
    };
    assert!(plan_release(&enabled, &RegistryState::default()).is_err());
}

#[cfg(unix)]
#[test]
fn literal_backslash_dependency_is_distinct_from_nested_directory() {
    use velnor_actions_rust::release_graph::plan_release;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers=['app','a/b']\nexclude=['a\\b']\nresolver='2'\n",
    );
    write(
        &root,
        "app/Cargo.toml",
        "[package]\nname='probe_app'\nversion='1.0.0'\nedition='2021'\n[dependencies]\nprobe_back={path='../a\\b',version='1'}\n",
    );
    write(&root, "app/src/lib.rs", "pub fn app() {}\n");
    write(
        &root,
        "a/b/Cargo.toml",
        "[package]\nname='probe_nested'\nversion='1.0.0'\nedition='2021'\n",
    );
    write(&root, "a/b/src/lib.rs", "pub fn nested() {}\n");
    write(
        &root,
        "a\\b/Cargo.toml",
        "[package]\nname='probe_back'\nversion='1.0.0'\nedition='2021'\n",
    );
    write(&root, "a\\b/src/lib.rs", "pub fn back() {}\n");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .env("CARGO_HOME", root.join("cargo-home"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("UTF-8");
    let scope = ReleaseScope::PublishableWorkspace;
    let empty = BTreeSet::new();
    let known = BTreeSet::from(["a\\b/Cargo.toml".to_owned()]);
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &known,
        supported_registries: &[],
    };
    let mut registry = RegistryState::default();
    registry.published.insert(
        velnor_actions_rust::RegistryPackage {
            registry: "crates-io".into(),
            package: "probe_back".into(),
        },
        BTreeSet::from(["1.0.0".into()]),
    );
    let plan =
        plan_release(&request, &registry).expect("excluded dependency is available from registry");
    assert!(
        plan.graph.edges.is_empty(),
        "excluded backslash path must not alias workspace member: {:?}",
        plan.graph.edges
    );
}

#[test]
fn dev_path_wildcard_presence_matches_cargo_package() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};
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
        assert!(metadata.status.success());
        let metadata = String::from_utf8(metadata.stdout).expect("utf8");
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
                .expect("git")
                .success()
        );
    }
    let path = dep.to_string_lossy().replace('\\', "/");
    let url = if cfg!(windows) {
        if let Some(unc) = path.strip_prefix("//?/UNC/") {
            format!("file://{unc}")
        } else {
            format!("file:///{}", path.strip_prefix("//?/").unwrap_or(&path))
        }
    } else {
        format!("file://{path}")
    };
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
