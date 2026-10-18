use super::*;
use crate::metadata::{TargetRecord, VelnorV1TaskOwner};
use velnor_actions_contract::config::{
    RustBinaryPreviewArchive, RustBinaryPreviewConfig, RustBinaryPreviewProducer,
    RustBinaryPreviewProduct,
};

fn preview(products: Vec<RustBinaryPreviewProduct>) -> RustBinaryPreviewConfig {
    RustBinaryPreviewConfig {
        enabled: true,
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryPreviewProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec![
                "Cargo.lock".to_owned(),
                "scripts/preview-producer.rs".to_owned(),
            ],
            ..RustBinaryPreviewProducer::default()
        },
        products,
        ..RustBinaryPreviewConfig::default()
    }
}

fn product(id: &str, package: &str, bins: &[&str], targets: &[&str]) -> RustBinaryPreviewProduct {
    RustBinaryPreviewProduct {
        id: id.to_owned(),
        package: package.to_owned(),
        bins: bins.iter().map(|value| (*value).to_owned()).collect(),
        targets: targets.iter().map(|value| (*value).to_owned()).collect(),
        archive: RustBinaryPreviewArchive::TarGz,
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

fn jackin_preview() -> RustBinaryPreviewConfig {
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
    let resolved = resolve_preview_products(&jackin_preview(), &jackin_workspace())?;
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
    assert!(resolve_preview_products(&missing_package, &jackin_workspace()).is_err());

    let missing_bin = preview(vec![product(
        "jackin",
        "jackin",
        &["ghost"],
        &["x86_64-unknown-linux-gnu"],
    )]);
    assert!(resolve_preview_products(&missing_bin, &jackin_workspace()).is_err());
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
    assert!(resolve_preview_products(&config, &first_workspace).is_err());

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
    assert!(resolve_preview_products(&config, &workspace).is_err());
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
    assert!(resolve_preview_products(&config, &feature_gated).is_err());

    let mut outside = package("jackin", "1.2.3", &[("jackin", &[])]);
    outside.in_workspace = false;
    let outside_id = outside.id.clone();
    let workspace = WorkspaceRecord {
        workspace_root: String::new(),
        members: Vec::new(),
        packages: vec![outside],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
    };
    assert!(workspace.members.iter().all(|id| id != &outside_id));
    assert!(resolve_preview_products(&config, &workspace).is_err());
}
