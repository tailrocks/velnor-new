// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use sha2::{Digest, Sha256};
use serde::Serialize;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciCandidateEvidence, OciImageConfiguration, inspect_oci_candidate_with_config,
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{AdmitRunnerImagesRequest, runner_profiles::ImageKind, source, validate};

const PLATFORM: &str = "linux/amd64";
const RUNNER_LOCK_PATH: &str = ".velnor/runner.lock";
const RUNNER_LOCK_LIMIT: usize = 1024 * 1024;

#[derive(Serialize)]
struct DescriptorDocument<'a> {
    schema_version: u8,
    runner_lock_sha256: &'a str,
    source_commit: &'a str,
    platform: &'static str,
    images: ImageEntries<'a>,
}

#[derive(Serialize)]
struct ImageEntries<'a> {
    runner: ImageEntry<'a>,
    dind: ImageEntry<'a>,
    resource_probe: ImageEntry<'a>,
}

#[derive(Serialize)]
struct ImageEntry<'a> {
    repository: &'static str,
    tag: String,
    registry_digest: &'a str,
    platform_manifest_digest: &'a str,
    config_digest: &'a str,
}

struct InspectedImage {
    evidence: OciCandidateEvidence,
}

pub(super) fn admit_runner_images(
    request: &AdmitRunnerImagesRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("runner_image_deadline"))?;
    let root = source::checked_root(&request.root)?;
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    validate_output_path(&root, &request.descriptors_output)?;
    let lock = read_exact_lock(&root, &request.source_commit, request.deadline)?;
    validate::validate_runner_lock(&lock)?;
    let runner_lock_sha256 = source::hex(&Sha256::digest(&lock));
    let runner = inspect_image(&request.runner_archive, ImageKind::Runner, &request.source_commit)?;
    let dind = inspect_image(&request.dind_archive, ImageKind::Dind, &request.source_commit)?;
    let probe = inspect_image(
        &request.probe_archive,
        ImageKind::ResourceProbe,
        &request.source_commit,
    )?;
    let bytes = serialize_descriptors(
        &request.source_commit,
        &runner_lock_sha256,
        &runner,
        &dind,
        &probe,
    )?;
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    if read_exact_lock(&root, &request.source_commit, request.deadline)? != lock {
        return Err(internal("runner_image_lock_changed"));
    }
    source::write_new_file(&request.descriptors_output, &bytes)
}

fn inspect_image(
    path: &Path,
    kind: ImageKind,
    source_commit: &str,
) -> Result<InspectedImage, OrchestratorError> {
    let inspection = inspect_oci_candidate_with_config(path)
        .map_err(|_| internal("runner_image_oci_invalid"))?;
    validate_image_configuration(kind, &inspection.configuration, source_commit)?;
    Ok(InspectedImage {
        evidence: inspection.evidence,
    })
}

fn validate_image_configuration(
    kind: ImageKind,
    configuration: &OciImageConfiguration,
    source_commit: &str,
) -> Result<(), OrchestratorError> {
    super::runner_profiles::validate_revision(configuration, source_commit)?;
    if matches!(kind, ImageKind::ResourceProbe) {
        super::runner_profiles::validate_probe_profile(configuration)?;
    }
    Ok(())
}

fn serialize_descriptors(
    source_commit: &str,
    runner_lock_sha256: &str,
    runner: &InspectedImage,
    dind: &InspectedImage,
    probe: &InspectedImage,
) -> Result<Vec<u8>, OrchestratorError> {
    let document = DescriptorDocument {
        schema_version: 2,
        runner_lock_sha256,
        source_commit,
        platform: PLATFORM,
        images: ImageEntries {
            runner: image_entry(ImageKind::Runner, &runner.evidence, source_commit),
            dind: image_entry(ImageKind::Dind, &dind.evidence, source_commit),
            resource_probe: image_entry(ImageKind::ResourceProbe, &probe.evidence, source_commit),
        },
    };
    let mut bytes = serde_json::to_vec(&document)
        .map_err(|_| internal("runner_image_descriptors_serialize"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn image_entry<'a>(
    kind: ImageKind,
    evidence: &'a OciCandidateEvidence,
    source_commit: &str,
) -> ImageEntry<'a> {
    ImageEntry {
        repository: kind.repository(),
        tag: format!("sha-{source_commit}"),
        registry_digest: &evidence.platform_manifest_digest,
        platform_manifest_digest: &evidence.platform_manifest_digest,
        config_digest: &evidence.image_configuration_digest,
    }
}

fn read_exact_lock(
    root: &Path,
    source_commit: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let committed = source::read_source_file(
        root,
        source_commit,
        RUNNER_LOCK_PATH,
        RUNNER_LOCK_LIMIT,
        deadline,
    )?;
    let working = validate::read_limited(
        &root.join(RUNNER_LOCK_PATH),
        RUNNER_LOCK_LIMIT as u64,
    )?;
    if committed != working {
        return Err(internal("runner_image_lock_source_mismatch"));
    }
    Ok(committed)
}

fn validate_output_path(root: &Path, path: &Path) -> Result<(), OrchestratorError> {
    if !path.is_absolute() || path.starts_with(root) {
        return Err(internal("runner_image_output_path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| internal("runner_image_output_parent"))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| internal("runner_image_output_parent"))?;
    if canonical_parent.starts_with(root) {
        return Err(internal("runner_image_output_path"));
    }
    Ok(())
}
