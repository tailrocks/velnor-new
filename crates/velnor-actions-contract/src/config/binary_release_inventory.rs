// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    RustBinaryReleaseCompanion, RustBinaryReleaseConfig, RustBinaryReleaseProduct, config_error,
    unique, valid_asset_name, valid_identifier, valid_repo_path,
};
use crate::errors::ContractError;
use crate::package_release::{
    MAX_PACKAGE_RELEASE_ARCHIVES, MAX_PACKAGE_RELEASE_ASSETS, MAX_PACKAGE_RELEASE_PRODUCTS,
};
use std::collections::BTreeSet;

pub(super) fn validate_inventory(
    file: &str,
    config: &RustBinaryReleaseConfig,
) -> Result<(), ContractError> {
    if !config.enabled() {
        return Ok(());
    }
    let manifest_prefix = format!("{}/", config.producer.output_directory);
    let manifest = config
        .producer
        .manifest
        .strip_prefix(&manifest_prefix)
        .ok_or_else(|| {
            config_error(
                file,
                "stacks.rust.binary_release.producer.manifest",
                "release_manifest_outside_output_directory",
            )
        })?;
    validate_static_release_inventory(file, &config.products, &config.companions, Some(manifest))
}

pub(crate) fn validate_static_release_inventory(
    file: &str,
    products: &[RustBinaryReleaseProduct],
    companions: &[RustBinaryReleaseCompanion],
    manifest_path: Option<&str>,
) -> Result<(), ContractError> {
    let prefix = "stacks.rust.binary_release";
    if products.is_empty() {
        return Err(ContractError::config(
            file,
            format!("{prefix}.products"),
            "required_when_enabled",
        ));
    }
    let product_count = products.len();
    let archives = products
        .iter()
        .map(|product| product.targets.len())
        .fold(0usize, usize::saturating_add);

    let receipt_assets = archives
        .saturating_mul(4)
        .saturating_add(companions.len().saturating_mul(2))
        .saturating_add(1);
    if product_count > MAX_PACKAGE_RELEASE_PRODUCTS
        || archives > MAX_PACKAGE_RELEASE_ARCHIVES
        || receipt_assets > MAX_PACKAGE_RELEASE_ASSETS
    {
        return Err(config_error(
            file,
            "stacks.rust.binary_release",
            "release_inventory_exceeds_receipt_limits",
        ));
    }
    validate_products(file, products)?;
    validate_companions(file, products, companions)?;
    validate_companion_paths(file, products, companions, manifest_path)
}

fn validate_products(
    file: &str,
    products: &[RustBinaryReleaseProduct],
) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    let mut packages = BTreeSet::new();
    for product in products {
        validate_product_identity(file, product, &mut ids, &mut packages)?;
        validate_product_outputs(file, product)?;
    }
    Ok(())
}

fn validate_product_identity(
    file: &str,
    product: &RustBinaryReleaseProduct,
    ids: &mut BTreeSet<String>,
    packages: &mut BTreeSet<String>,
) -> Result<(), ContractError> {
    let prefix = "stacks.rust.binary_release.products";
    if !valid_identifier(&product.id) || !ids.insert(product.id.clone()) {
        return Err(config_error(
            file,
            &format!("{prefix}.id"),
            "bad_or_duplicate_product_id",
        ));
    }
    if !valid_identifier(&product.package) || !packages.insert(product.package.clone()) {
        return Err(config_error(
            file,
            &format!("{prefix}.package"),
            "bad_or_duplicate_product_package",
        ));
    }
    Ok(())
}

fn validate_product_outputs(
    file: &str,
    product: &RustBinaryReleaseProduct,
) -> Result<(), ContractError> {
    let prefix = "stacks.rust.binary_release.products";
    if product.bins.is_empty()
        || product.bins.iter().any(|name| !valid_identifier(name))
        || !unique(&product.bins)
    {
        return Err(config_error(
            file,
            &format!("{prefix}.bins"),
            "bad_or_duplicate_binary_targets",
        ));
    }
    if product.targets.is_empty()
        || product
            .targets
            .iter()
            .any(|target| !supported_target(target))
        || !unique(&product.targets)
    {
        return Err(config_error(
            file,
            &format!("{prefix}.targets"),
            "bad_or_duplicate_rust_targets",
        ));
    }
    Ok(())
}

fn supported_target(target: &str) -> bool {
    crate::package_release::is_supported_release_target(target)
}

fn validate_companions(
    file: &str,
    products: &[RustBinaryReleaseProduct],
    companions: &[RustBinaryReleaseCompanion],
) -> Result<(), ContractError> {
    let mut ids = products
        .iter()
        .map(|product| product.id.clone())
        .collect::<BTreeSet<_>>();
    for companion in companions {
        validate_companion(file, companion, &mut ids)?;
    }
    let mut expected_assets = BTreeSet::new();
    for product in products {
        for target in &product.targets {
            let archive = format!("{}-{target}.tar.gz", product.id);
            for asset in [
                archive.clone(),
                format!("{archive}.sha256"),
                format!("{archive}.bundle"),
                format!("{archive}.sbom.json"),
            ] {
                if !expected_assets.insert(asset) {
                    return Err(config_error(
                        file,
                        "stacks.rust.binary_release.products",
                        "duplicate_or_reserved_release_asset_name",
                    ));
                }
            }
        }
    }
    for companion in companions {
        for asset in [
            companion.asset_name.clone(),
            format!("{}.bundle", companion.asset_name),
        ] {
            if !expected_assets.insert(asset) {
                return Err(config_error(
                    file,
                    "stacks.rust.binary_release.companions",
                    "duplicate_or_reserved_release_asset_name",
                ));
            }
        }
    }
    for asset in ["SHA256SUMS", "release.json", "release.json.bundle"] {
        if !expected_assets.insert(asset.to_owned()) {
            return Err(config_error(
                file,
                "stacks.rust.binary_release.companions",
                "duplicate_or_reserved_release_asset_name",
            ));
        }
    }
    if expected_assets.len()
        != products
            .iter()
            .map(|product| product.targets.len())
            .sum::<usize>()
            * 4
            + companions.len() * 2
            + 3
    {
        return Err(config_error(
            file,
            "stacks.rust.binary_release.companions",
            "duplicate_or_reserved_release_asset_name",
        ));
    }
    Ok(())
}

fn validate_companion(
    file: &str,
    companion: &RustBinaryReleaseCompanion,
    ids: &mut BTreeSet<String>,
) -> Result<(), ContractError> {
    let prefix = "stacks.rust.binary_release.companions";
    if !valid_identifier(&companion.id) || !ids.insert(companion.id.clone()) {
        return Err(config_error(
            file,
            &format!("{prefix}.id"),
            "bad_or_duplicate_companion_id",
        ));
    }
    if !valid_repo_path(&companion.path, false) || !valid_asset_name(&companion.asset_name) {
        return Err(config_error(file, prefix, "bad_companion_asset"));
    }
    Ok(())
}

fn validate_companion_paths(
    file: &str,
    products: &[RustBinaryReleaseProduct],
    companions: &[RustBinaryReleaseCompanion],
    manifest_path: Option<&str>,
) -> Result<(), ContractError> {
    let prefix = "stacks.rust.binary_release";
    let companion_paths = companions
        .iter()
        .map(|asset| asset.path.as_str())
        .collect::<Vec<_>>();
    for product_path in products.iter().flat_map(|product| {
        product
            .targets
            .iter()
            .map(move |target| format!("{}-{target}.tar.gz", product.id))
    }) {
        if companion_paths
            .iter()
            .any(|path| paths_overlap(path, &product_path))
        {
            return Err(config_error(
                file,
                &format!("{prefix}.companions.path"),
                "companion_path_conflicts_with_product_archive",
            ));
        }
    }
    if manifest_path.is_some_and(|manifest| {
        companion_paths
            .iter()
            .any(|path| paths_overlap(path, manifest))
    }) {
        return Err(config_error(
            file,
            &format!("{prefix}.companions.path"),
            "companion_path_conflicts_with_inventory_manifest",
        ));
    }
    if companion_paths.iter().enumerate().any(|(index, path)| {
        companion_paths
            .iter()
            .skip(index + 1)
            .any(|other| paths_overlap(path, other))
    }) {
        return Err(config_error(
            file,
            &format!("{prefix}.companions.path"),
            "overlapping_companion_paths",
        ));
    }
    Ok(())
}

pub(super) fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
