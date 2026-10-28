// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::{OrchestratorError, internal::internal};

use super::{
    AttestationExpectation, AttestationRunIdentity, PREDICATE, VerifiedAttestation, is_sha256,
    validate_certificate,
};

pub(super) fn validate_output_with_additional_names(
    bytes: &[u8],
    expected_subjects: &BTreeMap<String, String>,
    additional_subject_names: &[&str],
    expected: &AttestationExpectation<'_>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let attestations: Vec<VerifiedAttestation> = serde_json::from_slice(bytes)
        .map_err(|_| internal("package_attestation_output_invalid"))?;
    if attestations.is_empty() {
        return Err(internal("package_attestation_ambiguous"));
    }
    let expected_names = expected_subjects
        .keys()
        .map(String::as_str)
        .chain(additional_subject_names.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut current_subjects = None;
    for attestation in &attestations {
        let (invocation, subjects) =
            validate_attestation(attestation, expected_subjects, &expected_names, expected)?;
        if invocation.run_id == expected.run_id
            && invocation.run_attempt == expected.run_attempt
            && current_subjects.replace(subjects).is_some()
        {
            return Err(internal("package_attestation_ambiguous"));
        }
    }
    current_subjects.ok_or_else(|| internal("package_attestation_run_identity_mismatch"))
}

fn validate_attestation(
    attestation: &VerifiedAttestation,
    expected_subjects: &BTreeMap<String, String>,
    expected_names: &BTreeSet<&str>,
    expected: &AttestationExpectation<'_>,
) -> Result<(AttestationRunIdentity, BTreeMap<String, String>), OrchestratorError> {
    let verified = &attestation.verification_result;
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
    let found_names = found.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if expected_names != &found_names
        || expected_subjects
            .iter()
            .any(|(name, digest)| found.get(name) != Some(digest))
    {
        return Err(internal("package_attestation_subject_inventory_mismatch"));
    }
    let invocation = validate_certificate(&verified.signature.certificate, expected)?;
    Ok((invocation, found))
}
