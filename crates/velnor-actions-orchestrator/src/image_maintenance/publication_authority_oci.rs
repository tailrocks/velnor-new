// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ImageCandidateArchive, ImageCandidateOci};
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciCandidateEvidence, inspect_oci_candidate, inspect_oci_candidate_with_config,
};

use crate::{OrchestratorError, internal::internal};

use super::super::super::{ImagePublicationProfile, PublishImageRequest, runner_profiles};
use super::CandidateRecord;

pub(super) fn inspect_archive(
    request: &PublishImageRequest,
    candidate: &CandidateRecord,
) -> Result<OciCandidateEvidence, OrchestratorError> {
    let (actual, configuration) = request
        .deadline
        .scope(|| match candidate.profile {
            ImagePublicationProfile::Job => inspect_oci_candidate(&request.archive_path)
                .map(|evidence| (evidence, None)),
            ImagePublicationProfile::Runner
            | ImagePublicationProfile::Dind
            | ImagePublicationProfile::ResourceProbe => {
                inspect_oci_candidate_with_config(&request.archive_path)
                    .map(|inspection| (inspection.evidence, Some(inspection.configuration)))
            }
        })
        .map_err(|_| internal("image_publication_deadline"))?
        .map_err(|error| {
            if error.is_deadline_exceeded() {
                internal("image_publication_deadline")
            } else {
                internal("image_publication_oci_invalid")
            }
        })?;
    if !same_oci_evidence(&actual, &candidate.archive, &candidate.oci) {
        return Err(internal("image_publication_oci_attested_graph"));
    }
    if let Some(configuration) = configuration {
        runner_profiles::validate_revision(&configuration, &candidate.source_commit)?;
        if candidate.profile == ImagePublicationProfile::ResourceProbe {
            runner_profiles::validate_probe_profile(&configuration)?;
        }
    }
    Ok(actual)
}

fn same_oci_evidence(
    actual: &OciCandidateEvidence,
    archive: &ImageCandidateArchive,
    expected: &ImageCandidateOci,
) -> bool {
    actual.archive_sha256 == archive.sha256
        && actual.archive_size_bytes == archive.size_bytes
        && actual.archive_index_digest == expected.archive_index_digest
        && actual.archive_index_size_bytes == expected.archive_index_size_bytes
        && actual.platform_manifest_digest == expected.platform_manifest_digest
        && actual.platform_manifest_size_bytes == expected.platform_manifest_size_bytes
        && actual.image_configuration_digest == expected.image_configuration_digest
        && actual.image_configuration_size_bytes == expected.image_configuration_size_bytes
        && actual.layer_digests == expected.layer_digests
}
