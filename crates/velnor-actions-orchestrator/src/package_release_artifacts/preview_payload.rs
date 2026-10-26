// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{io::Read, path::Path, time::Instant};

use sha2::{Digest, Sha256};
use velnor_actions_contract::preview_production::{
    ReleaseProductionRequest, ReleaseProductionStage,
};
use velnor_actions_mise::CheckDeadline;

use super::{
    ArtifactKind, ExpectedFile, MAX_HANDOFF_BYTES, PackageOutputManifest, VerifiedPackageArtifact,
    archive, expected_files, invalid, inventory, validate_relative_path,
};
use crate::OrchestratorError;

#[path = "preview_payload_staging.rs"]
mod staging;

pub(crate) const PAYLOAD_OUTPUT_SCHEMA_V1: &str = "velnor.package-payload-output.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPayload {
    pub request: ReleaseProductionRequest,
    pub archive: VerifiedPackageArtifact,
    pub sbom: VerifiedPackageArtifact,
}

pub(crate) fn verify_payload_output(
    producer_dir: &Path,
    manifest_relative: &str,
    fresh_staging: &Path,
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
) -> Result<VerifiedPayload, OrchestratorError> {
    {
        verify_with_hook(
            producer_dir,
            manifest_relative,
            fresh_staging,
            request,
            deadline,
        )
    }
}

fn verify_with_hook(
    producer_dir: &Path,
    manifest_relative: &str,
    fresh_staging: &Path,
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
) -> Result<VerifiedPayload, OrchestratorError> {
    let mut guard = staging::StagingGuard::default();
    let result = deadline
        .scope(|| {
            let started = Instant::now();
            let expected = leaf_files(request, manifest_relative)?;
            checkpoint(deadline)?;
            let manifest = read_manifest(producer_dir, manifest_relative, request, started)?;
            inventory::compare_manifest_inventory(&manifest, &expected)?;
            inventory::validate_source_tree(producer_dir, &expected)?;
            checkpoint(deadline)?;
            guard.create(fresh_staging)?;
            let copied = inventory::copy_expected_files(
                producer_dir,
                fresh_staging,
                &expected,
                &manifest,
                started,
            )?;

            archive::validate_copied_content(
                &copied,
                &expected,
                &request.context.source_sha,
                &request.context.source_ref,
                &request.context.version,
                started,
            )?;
            recheck_staged_bytes(&copied, deadline)?;
            checkpoint(deadline)?;
            into_verified(copied, request)
        })
        .map_err(|_| invalid("preview_payload_deadline_exhausted"))
        .and_then(std::convert::identity);
    match result {
        Ok(verified) => {
            guard.disarm();
            Ok(verified)
        }
        Err(error) => {
            guard.cleanup()?;
            Err(error)
        }
    }
}

fn leaf_files(
    request: &ReleaseProductionRequest,
    manifest_relative: &str,
) -> Result<Vec<ExpectedFile>, OrchestratorError> {
    request
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    let ReleaseProductionStage::Payload { pair } = &request.stage else {
        return Err(invalid("preview_payload_stage_required"));
    };
    let manifest = validate_relative_path(manifest_relative)?;
    let expected = expected_files(&request.plan.products, &request.plan.companions, &manifest)?;
    let leaf = expected
        .into_iter()
        .filter(|file| {
            file.kind == ArtifactKind::Handoff
                || (file.product_id.as_deref() == Some(pair.product_id.as_str())
                    && file.target.as_deref() == Some(pair.target.as_str()))
        })
        .collect::<Vec<_>>();
    if leaf.len() != 3 {
        return Err(invalid("preview_payload_expected_inventory"));
    }
    Ok(leaf)
}

fn read_manifest(
    producer: &Path,
    manifest_relative: &str,
    request: &ReleaseProductionRequest,
    started: Instant,
) -> Result<PackageOutputManifest, OrchestratorError> {
    let relative = validate_relative_path(manifest_relative)?;
    inventory::ensure_no_symlink_path(producer, &relative)?;
    let bytes = inventory::read_limited(&producer.join(relative), MAX_HANDOFF_BYTES, started)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("handoff_not_utf8"))?;
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| invalid("handoff_invalid_json"))?;
    let manifest: PackageOutputManifest =
        serde_json::from_value(value).map_err(|_| invalid("handoff_schema_invalid"))?;
    if manifest.schema != PAYLOAD_OUTPUT_SCHEMA_V1
        || manifest.source_commit != request.context.source_sha
        || manifest.source_ref != request.context.source_ref
        || manifest.version != request.context.version
    {
        return Err(invalid("payload_handoff_source_schema_or_version_mismatch"));
    }
    Ok(manifest)
}

fn into_verified(
    copied: Vec<VerifiedPackageArtifact>,
    request: &ReleaseProductionRequest,
) -> Result<VerifiedPayload, OrchestratorError> {
    use velnor_actions_contract::package_release::PackageReleaseAssetKind;
    let mut archive = None;
    let mut sbom = None;
    for artifact in copied {
        match artifact.release_asset.kind {
            PackageReleaseAssetKind::Payload if archive.is_none() => archive = Some(artifact),
            PackageReleaseAssetKind::Sbom if sbom.is_none() => sbom = Some(artifact),
            _ => return Err(invalid("preview_payload_staged_inventory")),
        }
    }
    Ok(VerifiedPayload {
        request: request.clone(),
        archive: archive.ok_or_else(|| invalid("preview_payload_archive_missing"))?,
        sbom: sbom.ok_or_else(|| invalid("preview_payload_sbom_missing"))?,
    })
}

fn recheck_staged_bytes(
    copied: &[VerifiedPackageArtifact],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for artifact in copied {
        checkpoint(deadline)?;
        let mut file = inventory::open_regular_nofollow(&artifact.path)?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
        loop {
            checkpoint(deadline)?;
            let read = file.read(&mut buffer).map_err(|error| {
                OrchestratorError::io(artifact.path.display().to_string(), error.to_string())
            })?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > artifact.size {
                return Err(invalid("preview_payload_staged_mutation"));
            }
            hash.update(&buffer[..read]);
        }
        if total != artifact.size || inventory::hex_lower(&hash.finalize()) != artifact.sha256 {
            return Err(invalid("preview_payload_staged_mutation"));
        }
    }
    checkpoint(deadline)
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| invalid("preview_payload_deadline_exhausted"))
}
