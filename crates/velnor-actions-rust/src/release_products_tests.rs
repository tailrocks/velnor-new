use super::*;
use crate::metadata::{TargetRecord, VelnorV1TaskOwner};
use velnor_actions_contract::config::{
    RustBinaryReleaseArchive, RustBinaryReleaseConfig, RustBinaryReleaseProducer,
    RustBinaryReleaseProduct,
};

fn preview(products: Vec<RustBinaryReleaseProduct>) -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        version_package: "jackin".to_owned(),
        preview: velnor_actions_contract::config::RustBinaryPreviewConfig {
            enabled: true,
            ..velnor_actions_contract::config::RustBinaryPreviewConfig::default()
        },
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryReleaseProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec![
                "Cargo.lock".to_owned(),
                "scripts/preview-producer.rs".to_owned(),
            ],
            ..RustBinaryReleaseProducer::default()
        },
        products,
        ..RustBinaryReleaseConfig::default()
    }
}

fn product(id: &str, package: &str, bins: &[&str], targets: &[&str]) -> RustBinaryReleaseProduct {
    RustBinaryReleaseProduct {
        id: id.to_owned(),
        package: package.to_owned(),
        bins: bins.iter().map(|value| (*value).to_owned()).collect(),
        targets: targets.iter().map(|value| (*value).to_owned()).collect(),
        archive: RustBinaryReleaseArchive::TarGz,
    }
}

fn package(name: &str, version: &str, bins: &[(&str, &[&str])]) -> PackageRecord {
    let mut targets = bins
        .iter()
        .flat_map(|(name, required_features)| {
            vec![TargetRecord {
                kind: "bin".to_owned(),
                name: (*name).to_owned(),
                test: false,
                doctest: false,
                required_features: required_features
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
            }]
        })
        .collect::<Vec<_>>();
    targets.sort();
    PackageRecord {
        id: format!("{name} 1.0.0 (path+file:///repo/{name})"),
        name: name.to_owned(),
        version: version.to_owned(),
        manifest: format!("{name}/Cargo.toml"),
        external: false,
        in_workspace: true,
        targets,
        features: Vec::new(),
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    }
}

fn workspace(packages: Vec<PackageRecord>) -> WorkspaceRecord {
    let members = packages.iter().map(|package| package.id.clone()).collect();
    WorkspaceRecord {
        workspace_root: String::new(),
        members,
        packages,
        edges: Vec::new(),
        skipped_edges: Vec::new(),
        has_external_dependencies: false,
    }
}

fn jackin_workspace() -> WorkspaceRecord {
    workspace(vec![
        package(
            "jackin",
            "1.2.3",
            &[
                ("jackin", &[]),
                ("jackin-role", &[]),
                ("jackin-usage-broker", &[]),
            ],
        ),
        package("jackin-capsule", "1.2.3", &[("jackin-capsule", &[])]),
    ])
}

fn jackin_preview() -> RustBinaryReleaseConfig {
    preview(vec![
        product(
            "jackin",
            "jackin",
            &["jackin", "jackin-role", "jackin-usage-broker"],
            &[
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
            ],
        ),
        product(
            "jackin-capsule",
            "jackin-capsule",
            &["jackin-capsule"],
            &["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"],
        ),
    ])
}

#[test]
fn jackin_preview_resolves_six_archives_and_sorted_receipt_members()
-> Result<(), velnor_actions_contract::ContractError> {
    let resolved = resolve_release_products(&jackin_preview(), &jackin_workspace())?;
    assert_eq!(resolved.base_version, "1.2.3");
    assert_eq!(resolved.products.len(), 2);
    assert_eq!(resolved.products[0].id, "jackin");
    assert_eq!(
        resolved.products[0].bins,
        ["jackin", "jackin-role", "jackin-usage-broker"]
    );
    assert_eq!(resolved.products[0].payloads.len(), 4);
    assert_eq!(
        resolved.products[0].payloads[0].name,
        "jackin-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(
        resolved.products[0].payloads[0].members,
        resolved.products[0].bins
    );
    assert_eq!(resolved.products[1].id, "jackin-capsule");
    assert_eq!(resolved.products[1].payloads.len(), 2);
    assert!(
        resolved
            .products
            .iter()
            .flat_map(|product| &product.payloads)
            .all(|archive| archive.name.ends_with(".tar.gz"))
    );
    Ok(())
}

#[test]
fn missing_package_and_binary_fail_closed() {
    let missing_package = preview(vec![product(
        "missing",
        "missing",
        &["tool"],
        &["x86_64-unknown-linux-gnu"],
    )]);
    assert!(resolve_release_products(&missing_package, &jackin_workspace()).is_err());

    let missing_bin = preview(vec![product(
        "jackin",
        "jackin",
        &["ghost"],
        &["x86_64-unknown-linux-gnu"],
    )]);
    assert!(resolve_release_products(&missing_bin, &jackin_workspace()).is_err());
}

#[test]
fn package_versions_must_be_equal_numeric_bases() {
    let config = jackin_preview();
    let first_workspace = workspace(vec![
        package(
            "jackin",
            "1.2.3",
            &[
                ("jackin", &[]),
                ("jackin-role", &[]),
                ("jackin-usage-broker", &[]),
            ],
        ),
        package("jackin-capsule", "1.2.4", &[("jackin-capsule", &[])]),
    ]);
    assert!(resolve_release_products(&config, &first_workspace).is_err());

    let mut bad_version = package(
        "jackin",
        "1.2.3-rc.1",
        &[
            ("jackin", &[]),
            ("jackin-role", &[]),
            ("jackin-usage-broker", &[]),
        ],
    );
    bad_version.version = "1.2.3+build".to_owned();
    let workspace = workspace(vec![
        bad_version,
        package("jackin-capsule", "1.2.3", &[("jackin-capsule", &[])]),
    ]);
    assert!(resolve_release_products(&config, &workspace).is_err());
}

#[test]
fn feature_gated_and_nonworkspace_products_fail_closed() {
    let config = preview(vec![product(
        "jackin",
        "jackin",
        &["jackin"],
        &["x86_64-unknown-linux-gnu"],
    )]);
    let feature_gated = workspace(vec![package(
        "jackin",
        "1.2.3",
        &[("jackin", &["desktop"])],
    )]);
    assert!(resolve_release_products(&config, &feature_gated).is_err());

    let mut outside = package("jackin", "1.2.3", &[("jackin", &[])]);
    outside.in_workspace = false;
    let outside_id = outside.id.clone();
    let workspace = WorkspaceRecord {
        workspace_root: String::new(),
        members: Vec::new(),
        packages: vec![outside],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
        has_external_dependencies: false,
    };
    assert!(workspace.members.iter().all(|id| id != &outside_id));
    assert!(resolve_release_products(&config, &workspace).is_err());
}

#[test]
fn stable_and_preview_resolve_identical_source_bound_product_matrices() {
    let mut config = jackin_preview();
    let preview = resolve_release_products(&config, &jackin_workspace()).expect("preview products");
    config.preview.enabled = false;
    config.stable.enabled = true;
    config.products.reverse();
    let stable = resolve_release_products(&config, &jackin_workspace()).expect("stable products");
    assert_eq!(stable, preview);
    config.preview.enabled = false;
    config.stable.enabled = false;
    assert!(resolve_release_products(&config, &jackin_workspace()).is_err());
}

#[test]
fn version_owner_must_be_explicit_selected_and_present_in_discovered_workspace() {
    let mut config = jackin_preview();
    config.version_package = "not-a-product".to_owned();
    assert!(
        resolve_release_products(&config, &jackin_workspace())
            .expect_err("undeclared version owner")
            .to_string()
            .contains("version_package_not_a_product")
    );
    config.version_package = "jackin-capsule".to_owned();
    let mut incomplete = jackin_workspace();
    incomplete
        .packages
        .retain(|package| package.name != "jackin-capsule");
    assert!(
        resolve_release_products(&config, &incomplete)
            .expect_err("version owner absent")
            .to_string()
            .contains("missing_package")
    );
    assert_eq!(
        resolve_release_products(&config, &jackin_workspace())
            .expect("explicit alternate owner")
            .base_version,
        "1.2.3"
    );
}
