// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;

use super::{UntrustedProvenance, sha256};
use crate::owned_archive_guard::oci_candidate::{
    OciCandidateError, OciCandidateInspection, Result,
    archive::{authenticate_archive, load_archive_file},
    check_deadline, inspect_oci_candidate_file_with_config,
};

#[derive(Debug)]
pub struct UntrustedEnvelopeInspection {
    statement: Vec<u8>,
    candidate: OciCandidateInspection,
}

impl UntrustedEnvelopeInspection {
    #[must_use]
    pub fn statement(&self) -> &[u8] {
        &self.statement
    }

    #[must_use]
    pub fn candidate(&self) -> &OciCandidateInspection {
        &self.candidate
    }

    pub fn revalidate(&self, file: &File) -> Result<()> {
        let mut archive = load_archive_file(file)?;
        let (digest, size) = authenticate_archive(&mut archive)?;
        if digest != self.candidate.evidence.archive_sha256
            || size != self.candidate.evidence.archive_size_bytes
        {
            return Err(OciCandidateError("envelope_candidate_changed"));
        }
        check_deadline()
    }
}

pub fn verify_provenance_envelope(
    envelope: &UntrustedProvenance,
    candidate: &File,
) -> Result<UntrustedEnvelopeInspection> {
    check_deadline()?;
    velnor_runner_image_manifest::validate_provenance_envelope_limits(envelope)
        .map_err(|error| OciCandidateError(error.0))?;
    let index_digest = sha256(&envelope.buildkit_index);
    let manifest_digest = sha256(&envelope.attestation_manifest);
    let config_digest = sha256(&envelope.attestation_config);
    let statement_digest = sha256(&envelope.statement);
    let measured = velnor_runner_image_manifest::MeasuredEnvelopeDigests {
        buildkit_index_sha256: &index_digest,
        attestation_manifest_sha256: &manifest_digest,
        attestation_config_sha256: &config_digest,
        statement_sha256: &statement_digest,
    };
    let platform = velnor_runner_image_manifest::verify_envelope_graph(envelope, &measured)
        .map_err(|error| OciCandidateError(error.0))?;
    let inspection = inspect_oci_candidate_file_with_config(candidate)?;
    if inspection.manifest_media_type != platform.media_type
        || inspection.evidence.platform_manifest_digest != format!("sha256:{}", platform.digest)
        || inspection.evidence.platform_manifest_size_bytes != platform.size
    {
        return Err(OciCandidateError("envelope_candidate_identity"));
    }
    check_deadline()?;
    Ok(UntrustedEnvelopeInspection {
        statement: envelope.statement.clone(),
        candidate: inspection,
    })
}
