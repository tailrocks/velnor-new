use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::ContractError;
use velnor_actions_contract::config::{
    RustBinaryPreviewConfig, RustBinaryReleaseArchive, RustBinaryReleaseConfig,
    RustBinaryReleaseProducer, RustBinaryReleaseProduct,
};
use velnor_actions_rust::{project_release_workspace, release_workspace_manifest_paths};

const MEMBER_COUNT: usize = 153;

fn long_literal_workspace() -> (String, BTreeMap<String, Vec<u8>>, BTreeSet<String>, String) {
    let member_paths = (0..MEMBER_COUNT)
        .map(|index| {
            format!(
                "crates/{}/{}/{}/member-{index:03}",
                "domain".repeat(12),
                "subsystem".repeat(9),
                "component".repeat(9),
            )
        })
        .collect::<Vec<_>>();
    let mut root = String::from("[workspace]\nmembers = [\n");
    for path in &member_paths {
        root.push_str("  \"");
        root.push_str(path);
        root.push_str("\",\n");
    }
    root.push_str("]\n[workspace.package]\nversion = \"1.2.3\"\nedition = \"2024\"\n");

    let mut manifests = BTreeMap::from([("Cargo.toml".to_owned(), root.as_bytes().to_vec())]);
    let mut regular = BTreeSet::from(["Cargo.toml".to_owned()]);
    for (index, path) in member_paths.iter().enumerate() {
        let manifest_path = format!("{path}/Cargo.toml");
        let name = if index == 0 {
            "jackin".to_owned()
        } else {
            format!("member-{index:03}")
        };
        let manifest = format!(
            "[package]\nname = \"{name}\"\nversion.workspace = true\nedition.workspace = true\n"
        );
        manifests.insert(manifest_path.clone(), manifest.into_bytes());
        regular.insert(manifest_path);
    }
    let selected_manifest = format!("{}/Cargo.toml", member_paths[0]);
    regular.insert(format!("{}/src/main.rs", member_paths[0]));
    (root, manifests, regular, selected_manifest)
}

fn product_config() -> RustBinaryReleaseConfig {
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
            bins: vec!["jackin".to_owned()],
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            archive: RustBinaryReleaseArchive::TarGz,
        }],
        ..RustBinaryReleaseConfig::default()
    }
}

fn is_inventory_limit(error: &ContractError) -> bool {
    matches!(
        error,
        ContractError::InvalidIdentity { problem, .. } if problem == "workspace_inventory_limit"
    )
}

#[test]
fn long_literal_workspace_paths_project_without_glob_budget() {
    let (root, manifests, regular, selected_manifest) = long_literal_workspace();
    let paths = release_workspace_manifest_paths("Cargo.toml", root.as_bytes(), &regular)
        .expect("literal member paths");
    assert_eq!(paths.len(), MEMBER_COUNT + 1);
    assert!(paths.contains(&selected_manifest));

    let workspace = project_release_workspace(&product_config(), &manifests, &regular)
        .expect("product projection with long literal members");
    assert_eq!(workspace.packages.len(), 1);
    assert_eq!(workspace.packages[0].manifest, selected_manifest);
    assert_eq!(workspace.packages[0].version, "1.2.3");
}

#[test]
fn literal_and_glob_exclusions_filter_mixed_member_patterns() {
    let root = b"[workspace]\nmembers = [\"crates/*\", \"tools/special\"]\nexclude = [\"crates/private\", \"crates/ignored/**\"]\n";
    let regular = [
        "Cargo.toml",
        "crates/allowed/Cargo.toml",
        "crates/private/Cargo.toml",
        "crates/ignored/nested/Cargo.toml",
        "tools/special/Cargo.toml",
        "outside/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    assert_eq!(
        release_workspace_manifest_paths("Cargo.toml", root, &regular)
            .expect("mixed pattern paths"),
        [
            "Cargo.toml",
            "crates/allowed/Cargo.toml",
            "tools/special/Cargo.toml",
        ]
    );
}

#[test]
fn rejects_more_than_128_actual_glob_patterns() {
    let patterns = (0..129)
        .map(|index| format!("\"crates/entry-{index:03}*\""))
        .collect::<Vec<_>>()
        .join(",");
    let root = format!("[workspace]\nmembers = [{patterns}]\n");
    let regular = BTreeSet::from(["Cargo.toml".to_owned()]);

    let error = release_workspace_manifest_paths("Cargo.toml", root.as_bytes(), &regular)
        .expect_err("129 actual globs must exceed the bounded glob count");
    assert!(is_inventory_limit(&error));
}

#[test]
fn rejects_more_than_4096_raw_member_and_exclude_declarations() {
    let members = std::iter::repeat_n("\"crates/jackin\"", 4096)
        .collect::<Vec<_>>()
        .join(",");
    let root = format!("[workspace]\nmembers = [{members}]\nexclude = [\"crates/ignored\"]\n");
    let regular = BTreeSet::from([
        "Cargo.toml".to_owned(),
        "crates/jackin/Cargo.toml".to_owned(),
    ]);

    let error = release_workspace_manifest_paths("Cargo.toml", root.as_bytes(), &regular)
        .expect_err("raw duplicate declarations must remain bounded");
    assert!(is_inventory_limit(&error));
}
