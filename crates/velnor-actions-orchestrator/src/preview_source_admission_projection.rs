// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubApi, MAX_MANIFEST_CLOSURE_BYTES, SourceEntryMap, SourceFiles, WorkspaceSource,
    read_local, required,
};
use crate::{OrchestratorError, internal::internal};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_POLICY_SCHEMA_V1, PackageReleasePlanProduct, PackageReleasePolicy,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProducer},
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::expected_release_payload_pairs,
};

pub(super) fn derive_a_policy(
    api: &GitHubApi,
    workspace: &Path,
    release: &RustBinaryReleaseConfig,
    repository: &str,
    branch: &str,
) -> Result<
    (
        PackageReleasePolicy,
        velnor_actions_contract::PackageReleasePlan,
        SourceEntryMap,
    ),
    OrchestratorError,
> {
    let source_sha = required("VELNOR_RELEASE_SOURCE_SHA")?;
    let mut source = source_workspace_record(api, workspace, release, &source_sha)?;
    let resolved = velnor_actions_rust::resolve_release_products(release, &source.record)
        .map_err(|_| internal("preview_source_products"))?;
    let version = velnor_actions_rust::release_version_policy(release, &source.record)
        .map_err(|_| internal("preview_source_version_policy"))?;
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
        default_branch: branch.to_owned(),
        version,
        products,
        companions,
    };
    policy
        .validate()
        .map_err(|_| internal("preview_source_policy"))?;
    let plan = super::super::resolve_policy_at_source(&policy, &source_sha)?;
    if resolved.base_version.is_empty() || plan.base_version != resolved.base_version {
        return Err(internal("preview_source_policy_resolution"));
    }
    let mut entries = BTreeMap::new();
    for path in source.manifests.keys() {
        entries.insert(path.clone(), source.reader.identity(path)?);
    }
    Ok((policy, plan, entries))
}

fn source_workspace_record<'a>(
    api: &'a GitHubApi,
    workspace: &Path,
    release: &RustBinaryReleaseConfig,
    source_sha: &str,
) -> Result<WorkspaceSource<'a>, OrchestratorError> {
    let mut reader = SourceFiles::new(api, source_sha)?;
    let all_files = reader.regular_files()?;
    let regular_paths = all_files
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let root_manifest = reader.read(&release.manifest_path)?;
    let mut manifest_bytes = root_manifest.len();
    if read_local(workspace, &release.manifest_path)? != root_manifest {
        return Err(internal("preview_source_manifest_checkout_mismatch"));
    }
    let manifest_paths = velnor_actions_rust::release_workspace_manifest_paths(
        &release.manifest_path,
        &root_manifest,
        &regular_paths,
    )
    .map_err(|_| internal("preview_source_manifest_projection"))?;
    let mut manifests = BTreeMap::new();
    manifests.insert(release.manifest_path.clone(), root_manifest);
    for path in manifest_paths {
        if manifests.contains_key(&path) {
            continue;
        }
        let bytes = reader.read(&path)?;
        manifest_bytes = manifest_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_MANIFEST_CLOSURE_BYTES)
            .ok_or_else(|| internal("preview_source_manifest_closure_size"))?;
        if read_local(workspace, &path)? != bytes {
            return Err(internal("preview_source_manifest_checkout_mismatch"));
        }
        manifests.insert(path, bytes);
    }
    let workspace_record =
        velnor_actions_rust::project_release_workspace(release, &manifests, &regular_paths)
            .map_err(|_| internal("preview_source_workspace_projection"))?;
    Ok(WorkspaceSource {
        record: workspace_record,
        reader,
        manifests,
    })
}

pub(super) fn payload_matrix(
    plan: &velnor_actions_contract::PackageReleasePlan,
    producer: &RustBinaryReleaseProducer,
) -> Result<String, OrchestratorError> {
    let pairs = expected_release_payload_pairs(plan)
        .map_err(|_| internal("preview_source_payload_pairs"))?;
    if pairs.is_empty() || pairs.len() > 128 {
        return Err(internal("preview_source_payload_pairs"));
    }
    let timeout = u32::from(producer.timeout_minutes)
        + u32::from(RustBinaryReleaseProducer::JOB_HEADROOM_MINUTES);
    let rows = pairs
        .iter()
        .map(|pair| {
            serde_json::json!({
                "product_id": pair.product_id,
                "target": pair.target,
                "runner": producer.runner.runs_on(),
                "timeout_minutes": timeout,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({"include": rows});
    serde_json::to_string(&value).map_err(|_| internal("preview_source_matrix_encoding"))
}
