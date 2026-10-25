use crate::filesystem_owner::held_file::HeldFile;

use super::{
    handoff::{self, AssetProof, TaskInput},
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
        acquire_release_artifacts,
        artifact_zip::{self, ExpectedZipEntry, ZipProfile},
    },
};
use std::path::Path;
use velnor_actions_contract::{
    canonical_json_bytes,
    package_release::PackageReleasePayloadFormat,
    preview_production::{ReleaseProductionRequest, ReleaseProductionStage},
};
use velnor_actions_mise::CheckDeadline;

pub(super) struct AdmittedLeaves {
    pub(super) request: ReleaseProductionRequest,
    pub(super) manifests: Vec<PreviewLeafManifest>,
    pub(super) inputs: Vec<TaskInput>,
    pub(super) pinned: Vec<HeldFile>,
    pub(super) manifest_bytes: u64,
    pub(super) asset_bytes: u64,
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
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
) -> Result<AdmittedLeaves, OrchestratorError> {
    let root = handoff::fresh_directory(temp, "preview-leaves")?;
    let mut admitted = AdmittedLeaves {
        request: request.clone(),
        manifests: Vec::new(),
        inputs: Vec::new(),
        pinned: Vec::new(),
        manifest_bytes: 0,
        asset_bytes: 0,
    };
    let completed_request = acquire_release_artifacts(request, &root, deadline, |artifact| {
        admit_leaf(&root, request, deadline, artifact, &mut admitted)
    })?;
    complete_acquisition(admitted, request, completed_request, deadline)
}

pub(super) fn complete_acquisition(
    mut admitted: AdmittedLeaves,
    original: &ReleaseProductionRequest,
    completed: ReleaseProductionRequest,
    deadline: CheckDeadline,
) -> Result<AdmittedLeaves, OrchestratorError> {
    let mut expected = completed.clone();
    if original.plan.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
    {
        let ReleaseProductionStage::Assemble { inputs } = &original.stage else {
            return Err(internal("preview_acquisition_original_stage"));
        };
        if !inputs.is_empty() {
            return Err(internal("preview_acquisition_original_inputs"));
        }
        expected.stage = original.stage.clone();
    }
    if expected != *original || completed.validate().is_err() || admitted.request != *original {
        return Err(internal("preview_acquisition_completed_request"));
    }
    admitted.request = completed;
    admitted.inputs.sort_by(|a, b| a.name.cmp(&b.name));
    admitted.recheck(deadline)?;
    Ok(admitted)
}

fn admit_leaf(
    root: &Path,
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
    mut artifact: crate::package_release_publish::AdmittedReleaseArtifact,
    admitted: &mut AdmittedLeaves,
) -> Result<(), OrchestratorError> {
    let index = admitted.manifests.len();

    handoff::checkpoint(deadline)?;
    let mut expected = request.clone();
    expected.stage = ReleaseProductionStage::Payload {
        pair: artifact.reference.pair,
    };
    let bytes = artifact_zip::read_manifest_file(
        artifact.archive.reader()?,
        ZipProfile::PreviewLeaf,
        handoff::LEAF_MANIFEST,
        handoff::MAX_NATIVE_MANIFEST,
        deadline,
    )?;
    admitted.manifest_bytes = admitted
        .manifest_bytes
        .checked_add(bytes.len() as u64)
        .filter(|n| *n <= handoff::MAX_NATIVE_MANIFEST)
        .ok_or_else(|| internal("preview_leaf_metadata_total"))?;
    let leaf = PreviewLeafManifest::parse(&bytes, &expected)?;
    let raw = handoff::fresh_directory(root, &format!("raw-{index:03}"))?;
    let entries = leaf_entries(&leaf, &bytes);
    artifact_zip::extract_file(
        artifact.archive.reader()?,
        ZipProfile::PreviewLeaf,
        &raw,
        &entries,
        deadline,
    )?;
    artifact.archive.verify(deadline)?;
    let input = handoff::fresh_directory(root, &format!("input-{index:03}"))?;
    reconstruct_input(&raw, &input, &leaf, deadline)?;
    let verified = verify_payload_output(
        &input,
        "package-output.json",
        &root.join(format!("verified-{index:03}")),
        &expected,
        deadline,
    )?;
    retain_verified(admitted, &expected, verified, deadline)?;
    admitted.manifests.push(leaf);

    Ok(())
}

fn retain_verified(
    admitted: &mut AdmittedLeaves,
    expected: &ReleaseProductionRequest,
    verified: crate::package_release_artifacts::preview_payload::VerifiedPayload,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for asset in [verified.archive, verified.sbom] {
        admitted.asset_bytes = admitted
            .asset_bytes
            .checked_add(asset.size)
            .filter(|n| *n <= crate::package_release_artifacts::MAX_TOTAL_BYTES)
            .ok_or_else(|| internal("preview_leaf_asset_total"))?;
        let proof = AssetProof {
            name: asset.name.clone(),
            sha256: asset.sha256.clone(),
            size: asset.size,
        };
        admitted.pinned.push(HeldFile::new(
            &asset.path,
            &proof.sha256,
            proof.size,
            deadline,
        )?);
        let ReleaseProductionStage::Payload { pair } = &expected.stage else {
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

    Ok(())
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
        handoff::copy_verified(
            &raw.join(&asset.name),
            &input.join(&asset.name),
            asset,
            deadline,
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
    let ReleaseProductionStage::Payload { pair } = &leaf.request.stage else {
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
