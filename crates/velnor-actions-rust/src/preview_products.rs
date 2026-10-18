//! Resolve the configured binary preview products against Cargo metadata.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ContractError,
    config::{RustBinaryPreviewConfig, RustBinaryPreviewProduct},
    is_exact_semver,
    package_release::{PackageReleaseArchive, PackageReleaseProduct},
};

use crate::metadata::{PackageRecord, WorkspaceRecord};

/// Resolved preview package inventory and its shared numeric base version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPreviewProducts {
    /// Exact numeric Cargo package version shared by every product.
    pub base_version: String,
    /// Products sorted by receipt product ID, with sorted archive/member inventories.
    pub products: Vec<PackageReleaseProduct>,
}

/// Resolve preview products from one selected Cargo workspace's metadata.
///
/// This deliberately does not require Cargo publishability: binary products
/// may be private, workspace-only packages. All configured packages and bins
/// must resolve exactly within the selected workspace, and required-feature
/// bins fail closed because the preview product schema has no feature contract.
/// `config` must have passed [`RustBinaryPreviewConfig::validate`] first; the
/// schema layer owns path, target, uniqueness, and resource-limit validation.
///
/// # Errors
/// Returns a contract error when the preview policy, workspace membership,
/// package/bin inventory, or shared numeric version is invalid.
pub fn resolve_preview_products(
    config: &RustBinaryPreviewConfig,
    workspace: &WorkspaceRecord,
) -> Result<ResolvedPreviewProducts, ContractError> {
    if !config.enabled {
        return Err(invalid("preview_policy_disabled"));
    }
    if config.products.is_empty() {
        return Err(invalid("empty_product_inventory"));
    }

    let mut products = Vec::with_capacity(config.products.len());
    let mut package_names = BTreeSet::new();
    let mut base_version: Option<String> = None;

    for declared in &config.products {
        if !package_names.insert(declared.package.as_str()) {
            return Err(invalid("duplicate_product_package"));
        }
        let package = resolve_package(workspace, &declared.package)?;
        if !is_exact_semver(&package.version) {
            return Err(invalid("package_version_not_numeric_base"));
        }
        if base_version
            .as_deref()
            .is_some_and(|base| base != package.version)
        {
            return Err(invalid("package_version_mismatch"));
        }
        base_version.get_or_insert_with(|| package.version.clone());

        let bins = resolve_bins(package, declared)?;
        let mut targets = declared.targets.clone();
        targets.sort();
        let archives = targets
            .into_iter()
            .map(|target| PackageReleaseArchive {
                name: format!("{}-{target}.tar.gz", declared.id),
                target,
                members: bins.clone(),
            })
            .collect();
        products.push(PackageReleaseProduct {
            id: declared.id.clone(),
            package: declared.package.clone(),
            package_version: package.version.clone(),
            bins,
            archives,
        });
    }

    products.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ResolvedPreviewProducts {
        base_version: base_version.ok_or_else(|| invalid("empty_product_inventory"))?,
        products,
    })
}

fn resolve_package<'a>(
    workspace: &'a WorkspaceRecord,
    name: &str,
) -> Result<&'a PackageRecord, ContractError> {
    let candidates = workspace
        .packages
        .iter()
        .filter(|package| package.name == name)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(invalid("missing_package"));
    }
    let members = candidates
        .iter()
        .copied()
        .filter(|package| {
            package.in_workspace && workspace.members.iter().any(|id| id == &package.id)
        })
        .collect::<Vec<_>>();
    match members.as_slice() {
        [package] => Ok(package),
        [] => Err(invalid("package_not_in_selected_workspace")),
        _ => Err(invalid("ambiguous_workspace_package_name")),
    }
}

fn resolve_bins(
    package: &PackageRecord,
    declared: &RustBinaryPreviewProduct,
) -> Result<Vec<String>, ContractError> {
    let mut resolved = Vec::with_capacity(declared.bins.len());
    for bin in &declared.bins {
        let targets = package
            .targets
            .iter()
            .filter(|target| target.kind == "bin" && target.name == *bin)
            .collect::<Vec<_>>();
        match targets.as_slice() {
            [] => return Err(invalid("missing_binary_target")),
            [target] if !target.required_features.is_empty() => {
                return Err(invalid("binary_requires_features"));
            }
            [_] => resolved.push(bin.clone()),
            _ => return Err(invalid("ambiguous_binary_target")),
        }
    }
    resolved.sort();
    if resolved.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid("duplicate_binary_target"));
    }
    Ok(resolved)
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("preview_products", problem)
}

#[cfg(test)]
#[path = "preview_products_tests.rs"]
mod tests;
