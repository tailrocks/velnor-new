// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError,
    internal::internal,
    prepare::GenerationPreparation,
    release_identity::origin_repository,
};
use std::path::Path;
use velnor_actions_contract::{
    PackageReleasePlanProduct, package_release::PackageReleaseChannel,
    package_release::PackageReleasePayloadFormat,
};
use velnor_actions_rust::resolve_release_products;


pub(crate) fn release_policy(
    prep: &GenerationPreparation,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
) -> Result<velnor_actions_contract::PackageReleasePolicy, OrchestratorError> {
    let workspace_root = Path::new(&release.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let mut workspaces = prep
        .discovery
        .workspaces
        .iter()
        .filter(|workspace| workspace.record.workspace_root == workspace_root);
    let workspace = workspaces
        .next()
        .ok_or_else(|| internal("preview_workspace_missing"))?;
    if workspaces.next().is_some() {
        return Err(internal("preview_workspace_ambiguous"));
    }
    resolve_release_products(release, &workspace.record)?;
    let version = velnor_actions_rust::release_version_policy(release, &workspace.record)?;
    let mut products = release
        .products
        .iter()
        .map(|product| {
            let mut bins = product.bins.clone();
            bins.sort();
            let mut targets = product.targets.clone();
            targets.sort();
            PackageReleasePlanProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins,
                targets,
                format: PackageReleasePayloadFormat::TarGz,
            }
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    let mut companions = release.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    let policy = velnor_actions_contract::PackageReleasePolicy {
        schema: velnor_actions_contract::PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel,
        repository: origin_repository(&prep.root)?,
        default_branch: prep.default_branch.clone(),
        version,
        products,
        companions,
    };
    policy
        .validate()
        .map_err(|error| internal(&error.to_string()))?;
    Ok(policy)
}
