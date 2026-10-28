// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{OrchestratorError, internal::internal};

use super::{
    AttestationExpectation, PREDICATE, VerifiedAttestation, is_sha256, validate_certificate,
};

pub(super) fn validate_output_with_additional_names(
    bytes: &[u8],
    expected_subjects: &BTreeMap<String, String>,
    additional_subject_names: &[&str],
    expected: &AttestationExpectation<'_>,
) -> Result<(), OrchestratorError> {
    let attestations: Vec<VerifiedAttestation> = serde_json::from_slice(bytes)
        .map_err(|_| internal("package_attestation_output_invalid"))?;
    if attestations.len() != 1 {
        return Err(internal("package_attestation_ambiguous"));
    }
    let verified = &attestations[0].verification_result;
    if verified.statement.predicate_type != PREDICATE {
        return Err(internal("package_attestation_predicate_mismatch"));
    }
    if verified
        .statement
        .predicate
        .build_definition
        .internal_parameters
        .github
        .event_name
        != expected.trigger
    {
        return Err(internal("package_attestation_event_mismatch"));
    }
    let mut found = BTreeMap::new();
    for subject in &verified.statement.subject {
        let digest = subject
            .digest
            .get("sha256")
            .ok_or_else(|| internal("package_attestation_subject_digest_invalid"))?;
        if subject.digest.len() != 1
            || !is_sha256(digest)
            || found.insert(subject.name.clone(), digest.clone()).is_some()
        {
            return Err(internal("package_attestation_subject_inventory_invalid"));
        }
    }
    let expected_names = expected_subjects
        .keys()
        .map(String::as_str)
        .chain(additional_subject_names.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let found_names = found.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>();
    if expected_names != found_names
        || expected_subjects
            .iter()
            .any(|(name, digest)| found.get(name) != Some(digest))
    {
        return Err(internal("package_attestation_subject_inventory_mismatch"));
    }
    validate_certificate(&verified.signature.certificate, expected)
}
