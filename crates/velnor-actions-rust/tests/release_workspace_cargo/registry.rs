// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn cargo_workspace_reports_unsupported_sparse_registry_accurately() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("source");
    fs::create_dir(&root).expect("source");
    let root = root.canonicalize().expect("canonical source");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/excluded\"]\nresolver = \"2\"\n[workspace.package]\nversion = \"1.2.3-alpha.1\"\n",
    );
    write(
        &root,
        ".cargo/config.toml",
        "[registries.private]\nindex = \"sparse+https://example.invalid/index/\"\n",
    );
    write(
        &root,
        "crates/app/Cargo.toml",
        "[package]\nname=\"release_probe_app\"\nversion.workspace=true\nedition=\"2021\"\n[dependencies]\nrelease_probe_dep={path=\"../../hidden/dep\",version=\"0.1\"}\nexternal={version=\"1\",registry=\"private\"}\n",
    );
    write(&root, "crates/app/src/lib.rs", "pub fn app() {}\n");
    write(
        &root,
        "crates/excluded/Cargo.toml",
        "[package]\nname=\"release_probe_excluded\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    );
    write(
        &root,
        "crates/excluded/src/lib.rs",
        "pub fn excluded() {}\n",
    );
    write(
        &root,
        "hidden/dep/Cargo.toml",
        "[package]\nname=\"release_probe_dep\"\nversion=\"0.1.0\"\nedition=\"2021\"\npublish=false\n[build-dependencies]\nrelease_probe_more={path=\"../more\",version=\"0.1\"}\n",
    );
    write(&root, "hidden/dep/src/lib.rs", "pub fn dep() {}\n");
    write(
        &root,
        "hidden/more/Cargo.toml",
        "[package]\nname=\"release_probe_more\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    );
    write(&root, "hidden/more/src/lib.rs", "pub fn more() {}\n");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .env("CARGO_HOME", temp.path().join("cargo"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("utf8");
    let scope = ReleaseScope::Packages(vec!["release_probe_app".to_owned()]);
    let affected = BTreeSet::new();
    let supported = vec!["crates-io".to_owned()];
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &affected,
        supported_registries: &supported,
    };
    let selection =
        select_release_set(&request).expect("select Cargo workspace with a sparse registry");
    assert_eq!(selection.names(), vec!["release_probe_app"]);
    assert_eq!(selection.packages[0].version, "1.2.3-alpha.1");
    let record = parse_metadata_json(&json, &root, "Cargo.toml", &BTreeSet::new())
        .expect("workspace metadata");
    let error = publication_graph(&selection, &record, &RegistryState::default(), &supported)
        .expect_err("foreign registry cannot be published to crates.io");
    assert!(
        matches!(error, ReleaseError::UnsupportedRegistry { registry, .. } if registry == "sparse+https://example.invalid/index/")
    );
    let mut mismatched: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    let packages = mismatched["packages"].as_array_mut().expect("packages");
    let app = packages
        .iter_mut()
        .find(|p| p["name"] == "release_probe_app")
        .expect("app");
    let dependency = app["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .iter_mut()
        .find(|d| d["name"] == "external")
        .expect("external");
    dependency["registry"] = "sparse+https://other.invalid/index/".into();
    let mismatched = serde_json::to_string(&mismatched).expect("JSON");
    let mismatched_request = ReleaseRequest {
        metadata_json: &mismatched,
        ..request.clone()
    };
    assert!(matches!(
        select_release_set(&mismatched_request),
        Err(ReleaseError::MetadataMismatch { .. })
    ));
    let mut altered: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    let packages = altered["packages"].as_array_mut().expect("packages");
    let app = packages
        .iter_mut()
        .find(|p| p["name"] == "release_probe_app")
        .expect("app");
    let dependency = app["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .iter_mut()
        .find(|d| d["name"] == "external")
        .expect("external");
    dependency["registry"] = serde_json::Value::Null;
    let altered = serde_json::to_string(&altered).expect("JSON");
    let request = ReleaseRequest {
        metadata_json: &altered,
        ..request
    };
    let altered_selection = select_release_set(&request).expect("metadata selection");
    let altered_record =
        parse_metadata_json(&altered, &root, "Cargo.toml", &BTreeSet::new()).expect("metadata");
    let mut state = RegistryState::default();
    for (name, version) in [("external", "1.0.0"), ("release_probe_dep", "0.1.0")] {
        state.published.insert(
            velnor_actions_rust::RegistryPackage {
                registry: "crates-io".to_owned(),
                package: name.to_owned(),
            },
            BTreeSet::from([version.to_owned()]),
        );
    }
    let error = publication_graph(&altered_selection, &altered_record, &state, &supported)
        .expect_err("foreign source cannot be treated as crates.io");
    assert!(
        matches!(error, ReleaseError::UnsupportedRegistry { registry, .. } if registry == "sparse+https://example.invalid/index/")
    );
}

#[test]
fn crates_io_dependency_is_not_satisfied_by_another_registry() {
    use velnor_actions_rust::release_graph::RegistryPackage;

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(
        &root,
        "Cargo.toml",
        "[package]\nname=\"release_probe_app\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dependencies]\nexternal=\"1\"\n",
    );
    write(&root, "src/lib.rs", "pub fn app() {}\n");
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
    let scope = ReleaseScope::Packages(vec!["release_probe_app".to_owned()]);
    let affected = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &affected,
        supported_registries: &[],
    };
    let selection = select_release_set(&request).expect("selection");
    let record =
        parse_metadata_json(&json, &root, "Cargo.toml", &BTreeSet::new()).expect("metadata");
    let mut registry = RegistryState::default();
    registry.published.insert(
        RegistryPackage {
            registry: "private".to_owned(),
            package: "external".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    let error = publication_graph(&selection, &record, &registry, &[])
        .expect_err("other registry cannot satisfy crates.io dependency");
    assert!(matches!(error, ReleaseError::MissingRegistryVersion { dep, .. } if dep == "external"));
    registry.published.insert(
        RegistryPackage {
            registry: "crates-io".to_owned(),
            package: "external".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    assert!(publication_graph(&selection, &record, &registry, &[]).is_ok());
}

#[test]
fn release_plan_requires_published_unselected_dependency_and_orders_selected_packages() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers=[\"app\",\"dep\",\"helper\"]\nresolver=\"2\"\n",
    );
    write(
        &root,
        "app/Cargo.toml",
        "[package]\nname=\"plan_probe_app\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dependencies]\nplan_probe_dep={path=\"../dep\",version=\"1\",optional=true}\n[build-dependencies]\nplan_probe_helper={path=\"../helper\",version=\"1\"}\n",
    );
    write(
        &root,
        "dep/Cargo.toml",
        "[package]\nname=\"plan_probe_dep\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dev-dependencies]\nplan_probe_helper={path=\"../helper\",version=\"1\"}\n",
    );
    write(
        &root,
        "helper/Cargo.toml",
        "[package]\nname=\"plan_probe_helper\"\nversion=\"1.0.0\"\nedition=\"2021\"\npublish=false\n",
    );
    for package in ["app", "dep", "helper"] {
        write(
            &root,
            &format!("{package}/src/lib.rs"),
            "pub fn example() {}\n",
        );
    }
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
    let scope = ReleaseScope::Packages(vec![
        "plan_probe_app".to_owned(),
        "plan_probe_dep".to_owned(),
    ]);
    let affected = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &affected,
        supported_registries: &[],
    };
    let mut registry = RegistryState::default();
    let error = plan_release(&request, &registry)
        .expect_err("unpublished private helper must block release");
    assert!(
        matches!(error, ReleaseError::UnpublishedLocalDep { dep, .. } if dep == "plan_probe_helper")
    );
    registry.published.insert(
        RegistryPackage {
            registry: "crates-io".to_owned(),
            package: "plan_probe_helper".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    let plan = plan_release(&request, &registry).expect("published helper permits release");
    assert_eq!(plan.graph.order, ["plan_probe_dep", "plan_probe_app"]);
    assert_eq!(plan.graph.edges.len(), 1);
    assert!(plan.graph.edges[0].optional);
    assert_eq!(plan.graph.edges[0].to, "plan_probe_dep");
}

#[test]
fn plan_handles_versioned_dependencies_outside_workspace_and_omits_path_only_dev_dependencies() {
    use velnor_actions_rust::release_graph::{RegistryPackage, plan_release};
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical root");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers=[\"app\"]\nexclude=[\"outside\"]\nresolver=\"2\"\n[workspace.dependencies]\nrenamed={package=\"probe_external\",path=\"outside\",version=\"1\"}\n",
    );
    write(
        &root,
        "app/Cargo.toml",
        "[package]\nname=\"probe_app\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[target.'cfg(unix)'.dependencies]\nrenamed.workspace=true\n[dev-dependencies]\nprobe_dev={path=\"../outside/dev\"}\n",
    );
    write(&root, "app/src/lib.rs", "pub fn app() {}\n");
    write(
        &root,
        "outside/Cargo.toml",
        "[package]\nname=\"probe_external\"\nversion=\"1.0.0\"\nedition=\"2021\"\n",
    );
    write(&root, "outside/src/lib.rs", "pub fn outside() {}\n");
    write(
        &root,
        "outside/dev/Cargo.toml",
        "[package]\nname=\"probe_dev\"\nversion=\"1.0.0\"\nedition=\"2021\"\n",
    );
    write(&root, "outside/dev/src/lib.rs", "pub fn dev() {}\n");
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
    let affected = BTreeSet::new();
    let known = BTreeSet::from([
        "outside/Cargo.toml".to_owned(),
        "outside/dev/Cargo.toml".to_owned(),
    ]);
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &known,
        supported_registries: &[],
    };
    let mut registry = RegistryState::default();
    let error = plan_release(&request, &registry)
        .expect_err("versioned external dependency must exist in registry");
    assert!(
        matches!(&error, ReleaseError::MissingRegistryVersion { dep, .. } if dep == "probe_external"),
        "unexpected planner error: {error:?}"
    );
    registry.published.insert(
        RegistryPackage {
            registry: "crates-io".to_owned(),
            package: "probe_external".to_owned(),
        },
        BTreeSet::from(["1.0.0".to_owned()]),
    );
    let plan =
        plan_release(&request, &registry).expect("published external dependency permits release");
    assert_eq!(plan.graph.order, ["probe_app"]);
}
