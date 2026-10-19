//! Authenticate the full leaf union before unprivileged source assembly.

use super::{
    handoff::{self, AssetProof, PinnedAsset, TaskInput},
    leaf_manifest::PreviewLeafManifest,
};
use crate::{
    OrchestratorError,
    exclusive_write::write_exclusive_until,
    internal::internal,
    package_release_artifacts::{
        PackageOutputManifest, PackageOutputPayload, PackageOutputSbom,
        preview_payload::verify_payload_output,
    },
    package_release_publish::{
        acquire_preview_artifacts,
        artifact_zip::{self, ExpectedZipEntry},
    },
};
use std::path::Path;
use velnor_actions_contract::{
    canonical_json_bytes,
    package_release::PackageReleasePayloadFormat,
    preview_production::{PreviewProductionRequest, PreviewProductionStage},
};
use velnor_actions_mise::CheckDeadline;

pub(super) struct AdmittedLeaves {
    pub(super) manifests: Vec<PreviewLeafManifest>,
    pub(super) inputs: Vec<TaskInput>,
    pinned: Vec<PinnedAsset>,
}

impl AdmittedLeaves {
    pub(super) fn recheck(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        for asset in &self.pinned {
            asset.verify(deadline)?;
        }
        Ok(())
    }

    pub(super) fn require_unchanged_output(
        &self,
        assets: &[AssetProof],
    ) -> Result<(), OrchestratorError> {
        for leaf in &self.manifests {
            for original in &leaf.assets {
                if !assets.contains(original) {
                    return Err(internal("preview_assembly_replaced_leaf"));
                }
            }
        }
        Ok(())
    }
}

pub(super) fn acquire(
    temp: &Path,
    request: &PreviewProductionRequest,
    preview: &velnor_actions_contract::config::RustBinaryPreviewConfig,
    deadline: CheckDeadline,
) -> Result<AdmittedLeaves, OrchestratorError> {
    let artifacts = acquire_preview_artifacts(request, deadline)?;
    let root = handoff::fresh_directory(temp, "preview-leaves")?;
    let mut admitted = AdmittedLeaves {
        manifests: Vec::new(),
        inputs: Vec::new(),
        pinned: Vec::new(),
    };
    for (index, artifact) in artifacts.into_iter().enumerate() {
        handoff::checkpoint(deadline)?;
        let mut expected = request.clone();
        expected.stage = PreviewProductionStage::Payload {
            pair: artifact.reference.pair,
        };
        let bytes = artifact_zip::read_manifest(
            &artifact.bytes,
            handoff::LEAF_MANIFEST,
            handoff::MAX_NATIVE_MANIFEST,
            deadline,
        )?;
        let leaf = PreviewLeafManifest::parse(&bytes, &expected)?;
        leaf.validate_source(preview)?;
        let raw = handoff::fresh_directory(&root, &format!("raw-{index:03}"))?;
        let entries = leaf_entries(&leaf, &bytes);
        artifact_zip::extract(&artifact.bytes, &raw, &entries, deadline)?;
        let input = handoff::fresh_directory(&root, &format!("input-{index:03}"))?;
        reconstruct_input(&raw, &input, &leaf, deadline)?;
        let verified = verify_payload_output(
            &input,
            "package-output.json",
            &root.join(format!("verified-{index:03}")),
            &expected,
            deadline,
        )?;
        for asset in [verified.archive, verified.sbom] {
            let proof = AssetProof {
                name: asset.name.clone(),
                sha256: asset.sha256.clone(),
                size: asset.size,
            };
            admitted
                .pinned
                .push(PinnedAsset::new(&asset.path, &proof, deadline)?);
            let PreviewProductionStage::Payload { pair } = &expected.stage else {
                return Err(internal("preview_leaf_stage"));
            };
            admitted.inputs.push(TaskInput {
                pair: pair.clone(),
                name: proof.name,
                sha256: proof.sha256,
                size: proof.size,
                path: asset
                    .path
                    .to_str()
                    .ok_or_else(|| internal("preview_input_utf8"))?
                    .to_owned(),
            });
        }
        admitted.manifests.push(leaf);
    }
    admitted.inputs.sort_by(|a, b| a.name.cmp(&b.name));
    admitted.recheck(deadline)?;
    Ok(admitted)
}

fn leaf_entries(leaf: &PreviewLeafManifest, manifest: &[u8]) -> Vec<ExpectedZipEntry> {
    let mut entries = leaf
        .assets
        .iter()
        .map(|asset| ExpectedZipEntry {
            name: asset.name.clone(),
            size: Some(asset.size),
            sha256: Some(asset.sha256.clone()),
            max_bytes: asset.size,
            unix_permissions: Some(0o644),
        })
        .collect::<Vec<_>>();
    entries.push(ExpectedZipEntry {
        name: handoff::LEAF_MANIFEST.to_owned(),
        size: Some(manifest.len() as u64),
        sha256: Some(handoff::sha256(manifest)),
        max_bytes: handoff::MAX_NATIVE_MANIFEST,
        unix_permissions: Some(0o644),
    });
    entries
}

fn reconstruct_input(
    raw: &Path,
    input: &Path,
    leaf: &PreviewLeafManifest,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for asset in &leaf.assets {
        let bytes = handoff::read_verified(&raw.join(&asset.name), asset, deadline)?;
        write_exclusive_until(
            &input.join(&asset.name),
            &bytes,
            "preview_leaf_input",
            || handoff::checkpoint(deadline),
        )?;
    }
    let manifest = source_manifest(leaf)?;
    let bytes =
        canonical_json_bytes(&manifest).map_err(|_| internal("preview_leaf_handoff_encode"))?;
    write_exclusive_until(
        &input.join("package-output.json"),
        &bytes,
        "preview_leaf_handoff",
        || handoff::checkpoint(deadline),
    )
}

fn source_manifest(leaf: &PreviewLeafManifest) -> Result<PackageOutputManifest, OrchestratorError> {
    let PreviewProductionStage::Payload { pair } = &leaf.request.stage else {
        return Err(internal("preview_leaf_stage"));
    };
    let archive = format!("{}-{}.tar.gz", pair.product_id, pair.target);
    let payload = leaf
        .assets
        .iter()
        .find(|row| row.name == archive)
        .ok_or_else(|| internal("preview_leaf_archive_missing"))?;
    let sbom = leaf
        .assets
        .iter()
        .find(|row| row.name == format!("{archive}.sbom.json"))
        .ok_or_else(|| internal("preview_leaf_sbom_missing"))?;
    Ok(PackageOutputManifest {
        schema: "velnor.package-payload-output.v1".to_owned(),
        source_commit: leaf.request.context.source_sha.clone(),
        source_ref: leaf.request.context.source_ref.clone(),
        version: leaf.request.context.version.clone(),
        payloads: vec![PackageOutputPayload {
            product_id: pair.product_id.clone(),
            target: pair.target.clone(),
            format: PackageReleasePayloadFormat::TarGz,
            name: payload.name.clone(),
            sha256: payload.sha256.clone(),
        }],
        companions: Vec::new(),
        sboms: vec![PackageOutputSbom {
            payload_name: archive,
            name: sbom.name.clone(),
            sha256: sbom.sha256.clone(),
        }],
    })
}
