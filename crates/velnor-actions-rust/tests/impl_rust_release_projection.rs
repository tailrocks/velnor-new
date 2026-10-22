use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::config::{
    RustBinaryPreviewConfig, RustBinaryReleaseArchive, RustBinaryReleaseConfig,
    RustBinaryReleaseProducer, RustBinaryReleaseProduct,
};
use velnor_actions_rust::{
    project_release_workspace, release_workspace_manifest_paths, resolve_release_products,
};

const ROOT: &str = r#"
[workspace]
members = ["crates/*"]
exclude = ["crates/ignored"]
resolver = "3"

[workspace.package]
version = "1.2.3"
edition = "2024"
"#;

const JACKIN: &str = r#"
[package]
name = "jackin"
version.workspace = true
edition.workspace = true
workspace = "../.."

[[bin]]
name = "jackin"
path = "src/main.rs"

[[bin]]
name = "jackin-role"
path = "src/role.rs"
required-features = ["role"]
"#;

fn config(bins: &[&str]) -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        version_package: "jackin".to_owned(),
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        preview: RustBinaryPreviewConfig {
            enabled: true,
            ..RustBinaryPreviewConfig::default()
        },
        producer: RustBinaryReleaseProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec!["Cargo.lock".to_owned(), "mise.toml".to_owned()],
            ..RustBinaryReleaseProducer::default()
        },
        products: vec![RustBinaryReleaseProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: bins.iter().map(|bin| (*bin).to_owned()).collect(),
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            archive: RustBinaryReleaseArchive::TarGz,
        }],
        ..RustBinaryReleaseConfig::default()
    }
}

fn base_inventory() -> (BTreeMap<String, Vec<u8>>, BTreeSet<String>) {
    let mut manifests = BTreeMap::new();
    manifests.insert("Cargo.toml".to_owned(), ROOT.as_bytes().to_vec());
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        JACKIN.as_bytes().to_vec(),
    );
    let regular = [
        "Cargo.toml",
        "crates/jackin/Cargo.toml",
        "crates/jackin/src/main.rs",
        "crates/jackin/src/role.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    (manifests, regular)
}

#[test]
fn projects_a_only_manifest_inventory_and_inherited_package_version() {
    let (manifests, regular) = base_inventory();
    let paths = release_workspace_manifest_paths("Cargo.toml", ROOT.as_bytes(), &regular)
        .expect("member paths");
    assert_eq!(paths, ["Cargo.toml", "crates/jackin/Cargo.toml"]);
    let workspace =
        project_release_workspace(&config(&["jackin"]), &manifests, &regular).expect("projection");
    let package = workspace.packages.first().expect("selected package");
    assert_eq!(package.name, "jackin");
    assert_eq!(package.version, "1.2.3");
    assert_eq!(package.targets[0].name, "jackin");
    assert!(workspace.has_external_dependencies);
}

#[test]
fn workspace_glob_exclusions_are_applied_to_authenticated_paths() {
    let root = r#"[workspace]
members = ["crates/**"]
exclude = ["crates/ignored"]
"#;
    let regular = [
        "Cargo.toml",
        "crates/a/Cargo.toml",
        "crates/nested/b/Cargo.toml",
        "crates/ignored/Cargo.toml",
        "elsewhere/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let paths = release_workspace_manifest_paths("Cargo.toml", root.as_bytes(), &regular)
        .expect("member paths");
    assert_eq!(
        paths,
        [
            "Cargo.toml",
            "crates/a/Cargo.toml",
            "crates/nested/b/Cargo.toml"
        ]
    );
}

#[test]
fn double_star_directory_glob_requires_a_directory_boundary() {
    let root = b"[workspace]\nmembers = [\"crates/**/app\"]\n";
    let regular = [
        "Cargo.toml",
        "crates/app/Cargo.toml",
        "crates/foo/app/Cargo.toml",
        "crates/fooapp/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        release_workspace_manifest_paths("Cargo.toml", root, &regular).expect("member paths"),
        [
            "Cargo.toml",
            "crates/app/Cargo.toml",
            "crates/foo/app/Cargo.toml"
        ]
    );
}

#[test]
fn root_package_can_be_a_literal_workspace_member() {
    let root = b"[package]\nname='jackin'\nversion='1.2.3'\n[workspace]\nmembers=['.']\n";
    let regular = ["Cargo.toml"].into_iter().map(str::to_owned).collect();
    assert_eq!(
        release_workspace_manifest_paths("Cargo.toml", root, &regular).expect("root member"),
        ["Cargo.toml"]
    );
}

#[test]
fn rejects_missing_literal_members_and_bounds_adversarial_glob_work() {
    let missing = b"[workspace]\nmembers = [\"crates/missing\"]\n";
    let regular = ["Cargo.toml"].into_iter().map(str::to_owned).collect();
    assert!(release_workspace_manifest_paths("Cargo.toml", missing, &regular).is_err());

    let patterns = (0..24)
        .map(|_| format!("\"{}b\"", "*a".repeat(400)))
        .collect::<Vec<_>>()
        .join(",");
    let adversarial = format!("[workspace]\nmembers = [{patterns}]\n");
    let regular = [
        "Cargo.toml".to_owned(),
        format!("{}/Cargo.toml", "a".repeat(970)),
    ]
    .into_iter()
    .collect();
    assert!(
        release_workspace_manifest_paths("Cargo.toml", adversarial.as_bytes(), &regular).is_err()
    );
}

#[test]
fn rejects_missing_nonregular_ambiguous_and_path_escaping_binary_sources() {
    let (manifests, mut regular) = base_inventory();
    regular.remove("crates/jackin/src/main.rs");
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());

    let (manifests, mut regular) = base_inventory();
    regular.insert("crates/jackin/src/main.rs/../main.rs".to_owned());
    assert!(release_workspace_manifest_paths("../Cargo.toml", ROOT.as_bytes(), &regular).is_err());
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());

    let mut manifests = manifests;
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
autobins = false
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());

    let mut manifests = base_inventory().0;
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true

[[bin]]
name = "jackin"
path = "../../outside/main.rs"
"#
        .to_vec(),
    );
    assert!(
        project_release_workspace(&config(&["jackin"]), &manifests, &base_inventory().1).is_err()
    );
}

#[test]
fn rejects_feature_gated_and_ambiguous_automatic_bins() {
    let (manifests, regular) = base_inventory();
    let workspace = project_release_workspace(&config(&["jackin-role"]), &manifests, &regular)
        .expect("feature-gated target is retained for policy validation");
    assert!(resolve_release_products(&config(&["jackin-role"]), &workspace).is_err());

    let mut regular = regular;
    regular.insert("crates/jackin/src/bin/jackin.rs".to_owned());
    let mut manifests = manifests;
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}

#[test]
fn explicit_bin_paths_are_inferred_even_when_autobins_are_disabled() {
    let (mut manifests, mut regular) = base_inventory();
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
autobins = false

[[bin]]
name = "jackin"
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_ok());

    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true

[[bin]]
name = "jackin"
        path = "/src/main.rs"
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());

    regular.insert("crates/jackin/build.rs".to_owned());
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
build = "/build.rs"

[[bin]]
name = "jackin"
path = "src/main.rs"
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}

#[test]
fn edition_2015_manual_bin_disables_undeclared_auto_bins() {
    let (mut manifests, regular) = base_inventory();
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true

[[bin]]
name = "manual"
path = "src/main.rs"
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}

#[test]
fn rejects_duplicate_member_names_and_mismatched_workspace_ownership() {
    let (mut manifests, regular) = base_inventory();
    manifests.insert(
        "crates/other/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
"#
        .to_vec(),
    );
    let mut regular = regular;
    regular.insert("crates/other/Cargo.toml".to_owned());
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());

    let (mut manifests, regular) = base_inventory();
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        br#"[package]
name = "jackin"
version.workspace = true
workspace = "../other"
"#
        .to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}

#[test]
fn rejects_workspace_parent_escape_but_accepts_explicit_root_workspace() {
    let (mut manifests, regular) = base_inventory();
    manifests.insert(
        "crates/jackin/Cargo.toml".to_owned(),
        JACKIN.replace("../..", "../../../").into_bytes(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}

#[test]
fn rejects_nearer_nested_workspace_boundaries() {
    let (mut manifests, mut regular) = base_inventory();
    regular.insert("crates/Cargo.toml".to_owned());
    manifests.insert(
        "crates/Cargo.toml".to_owned(),
        b"[workspace]\nmembers = [\"jackin\"]\n".to_vec(),
    );
    assert!(project_release_workspace(&config(&["jackin"]), &manifests, &regular).is_err());
}
