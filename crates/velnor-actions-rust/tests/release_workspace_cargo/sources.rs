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
