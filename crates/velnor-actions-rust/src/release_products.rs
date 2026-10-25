use std::collections::BTreeSet;

use velnor_actions_contract::{
    ContractError,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProduct},
    is_exact_semver,
    package_release::{PackageReleasePayload, PackageReleaseProduct},
};

use crate::metadata::{PackageRecord, WorkspaceRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReleaseProducts {
    pub base_version: String,
    pub products: Vec<PackageReleaseProduct>,
}

pub fn resolve_release_products(
    config: &RustBinaryReleaseConfig,
    workspace: &WorkspaceRecord,
) -> Result<ResolvedReleaseProducts, ContractError> {
    if !config.enabled() {
        return Err(invalid("release_policy_disabled"));
    }
    if config.products.is_empty() {
        return Err(invalid("empty_product_inventory"));
    }

    let mut products = Vec::with_capacity(config.products.len());
    let mut package_names = BTreeSet::new();
    if !config
        .products
        .iter()
        .any(|product| product.package == config.version_package)
    {
        return Err(invalid("version_package_not_a_product"));
    }
    let version_owner = resolve_package(workspace, &config.version_package)?;
    if !is_exact_semver(&version_owner.version) {
        return Err(invalid("package_version_not_numeric_base"));
    }
    let base_version = version_owner.version.clone();

    for declared in &config.products {
        if !package_names.insert(declared.package.as_str()) {
            return Err(invalid("duplicate_product_package"));
        }
        let package = resolve_package(workspace, &declared.package)?;
        if !is_exact_semver(&package.version) {
            return Err(invalid("package_version_not_numeric_base"));
        }
        if base_version != package.version {
            return Err(invalid("package_version_mismatch"));
        }

        let bins = resolve_bins(package, declared)?;
        let mut targets = declared.targets.clone();
        targets.sort();
        let payloads = targets
            .into_iter()
            .map(|target| PackageReleasePayload {
                format:
                    velnor_actions_contract::package_release::PackageReleasePayloadFormat::TarGz,
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
            payloads,
        });
    }

    products.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ResolvedReleaseProducts {
        base_version,
        products,
    })
}

pub fn release_version_policy(
    config: &RustBinaryReleaseConfig,
    workspace: &WorkspaceRecord,
) -> Result<velnor_actions_contract::CargoVersionPolicy, ContractError> {
    let names = config
        .products
        .iter()
        .map(|product| product.package.as_str())
        .collect::<BTreeSet<_>>();
    let bindings = names
        .iter()
        .map(|name| {
            let package = resolve_package(workspace, name)?;
            Ok(velnor_actions_contract::CargoVersionBinding {
                package: (*name).to_owned(),
                manifest_path: package.manifest.clone(),
                workspace_manifest_path: config.manifest_path.clone(),
            })
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    let version = velnor_actions_contract::CargoVersionPolicy {
        version_package: config.version_package.clone(),
        bindings,
    };
    version.validate(&names)?;
    Ok(version)
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
    declared: &RustBinaryReleaseProduct,
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
    ContractError::identity("release_products", problem)
}
