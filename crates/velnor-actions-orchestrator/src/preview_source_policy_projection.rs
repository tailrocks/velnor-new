// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{
    PACKAGE_RELEASE_POLICY_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    PackageReleasePolicy,
    config::RustBinaryReleaseConfig,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    package_release_policy::CargoVersionPolicy,
};

use super::super::github::GitHubApi;
use super::{resolve_policy_at_source, source_files::SourceFiles};

const MAX_SOURCE_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

pub(in crate::package_release_publish) fn read_preview_policy_at_source(
    api: &GitHubApi,
    source_sha: &str,
    repository: &str,
    default_branch: &str,
) -> Result<(PackageReleasePolicy, PackageReleasePlan), OrchestratorError> {
    if !valid_git_sha(source_sha) || repository.is_empty() || default_branch != "main" {
        return Err(internal("preview_source_policy_context"));
    }
    let mut source = SourceFiles::new(api, source_sha)?;
    let regular_paths = source
        .regular_files()?
        .into_iter()
        .map(|entry| entry.path)
        .collect::<BTreeSet<_>>();
    let config = read_source_config(&mut source)?;
    let release = source_release_config(&config)?;
    let manifests = source_workspace_manifests(&mut source, &release, &regular_paths)?;
    let workspace =
        velnor_actions_rust::project_release_workspace(&release, &manifests, &regular_paths)
            .map_err(|_| internal("preview_source_workspace_projection"))?;
    let resolved = velnor_actions_rust::resolve_release_products(&release, &workspace)
        .map_err(|_| internal("preview_source_products"))?;
    let version = velnor_actions_rust::release_version_policy(&release, &workspace)
        .map_err(|_| internal("preview_source_version_policy"))?;
    let policy = source_policy(&release, repository, default_branch, version)?;
    let plan = resolve_policy_at_source(&policy, source_sha)?;
    if resolved.base_version.is_empty() || plan.base_version != resolved.base_version {
        return Err(internal("preview_source_policy_resolution"));
    }
    Ok((policy, plan))
}

fn read_source_config(
    source: &mut SourceFiles<'_>,
) -> Result<velnor_actions_contract::config::VelnorConfig, OrchestratorError> {
    let bytes = source.read(".velnor/config.toml")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("preview_source_config_utf8"))?;
    crate::config::load_config_from_text(text)
}

fn source_release_config(
    config: &velnor_actions_contract::config::VelnorConfig,
) -> Result<RustBinaryReleaseConfig, OrchestratorError> {
    let release = config
        .stacks
        .rust
        .as_ref()
        .map(|rust| rust.binary_release.clone())
        .ok_or_else(|| internal("preview_source_config_missing"))?;
    release.validate(".velnor/config.toml")?;
    if !release.channel_enabled(PackageReleaseChannel::Preview) {
        return Err(internal("preview_source_channel_disabled"));
    }
    Ok(release)
}

fn source_workspace_manifests(
    source: &mut SourceFiles<'_>,
    release: &RustBinaryReleaseConfig,
    regular_paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let root = source.read(&release.manifest_path)?;
    std::str::from_utf8(&root).map_err(|_| internal("preview_source_manifest_utf8"))?;
    let paths = velnor_actions_rust::release_workspace_manifest_paths(
        &release.manifest_path,
        &root,
        regular_paths,
    )
    .map_err(|_| internal("preview_source_manifest_projection"))?;
    let mut manifests = BTreeMap::new();
    manifests.insert(release.manifest_path.clone(), root);
    let mut total_bytes = manifests[&release.manifest_path].len();
    if total_bytes > MAX_SOURCE_MANIFEST_BYTES {
        return Err(internal("preview_source_manifest_closure_size"));
    }
    for path in paths {
        if manifests.contains_key(&path) {
            continue;
        }
        let bytes = source.read(&path)?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_SOURCE_MANIFEST_BYTES)
            .ok_or_else(|| internal("preview_source_manifest_closure_size"))?;
        manifests.insert(path, bytes);
    }
    Ok(manifests)
}

fn source_policy(
    release: &RustBinaryReleaseConfig,
    repository: &str,
    default_branch: &str,
    version: CargoVersionPolicy,
) -> Result<PackageReleasePolicy, OrchestratorError> {
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
    let policy = PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: repository.to_owned(),
        default_branch: default_branch.to_owned(),
        version,
        products,
        companions,
    };
    policy
        .validate()
        .map_err(|_| internal("preview_source_policy"))?;
    Ok(policy)
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
