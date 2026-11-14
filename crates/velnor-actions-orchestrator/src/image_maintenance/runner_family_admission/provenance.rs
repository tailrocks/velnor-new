// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;

use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciCandidateInspection, UntrustedEnvelopeInspection, admits_image_configuration,
    parse_provenance_envelope, verify_provenance_envelope,
};

use super::{ExpectedBuildSource, admits_build_source};

#[derive(Debug)]
pub(crate) struct StructurallyVerifiedProvenance {
    inspection: UntrustedEnvelopeInspection,
}

impl StructurallyVerifiedProvenance {
    #[must_use]
    pub(crate) fn candidate(&self) -> &OciCandidateInspection {
        self.inspection.candidate()
    }

    #[must_use]
    pub(crate) fn revalidate(&self, file: &File) -> bool {
        self.inspection.revalidate(file).is_ok()
    }
}

pub(crate) fn verify_family_provenance(
    candidate: &File,
    envelope_bytes: &[u8],
    expected: &ExpectedBuildSource<'_>,
) -> Option<StructurallyVerifiedProvenance> {
    if velnor_actions_mise::CheckDeadline::check_scope().is_err() {
        return None;
    }
    let envelope = parse_provenance_envelope(envelope_bytes).ok()?;
    let verified = verify_provenance_envelope(&envelope, candidate).ok()?;
    let inspection = verified.candidate();
    if inspection.index_has_annotations
        || inspection.evidence.platform_manifest_digest != expected.platform_digest
        || !admits_image_configuration(
            &inspection.configuration,
            expected.role,
            expected.source_commit,
        )
        || !admits_build_source(verified.statement(), expected)
        || velnor_actions_mise::CheckDeadline::check_scope().is_err()
    {
        return None;
    }
    verified.revalidate(candidate).ok()?;
    Some(StructurallyVerifiedProvenance {
        inspection: verified,
    })
}
