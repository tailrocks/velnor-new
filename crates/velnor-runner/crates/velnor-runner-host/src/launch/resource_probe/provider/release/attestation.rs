//! Exact signer and artifact subject validation for release attestations.

use std::collections::HashSet;

use serde_json::Value;

use super::super::{OIDC_ISSUER, ProviderFailure, SIGNER_IDENTITY, SOURCE_REF, SOURCE_REPOSITORY};

#[cfg(test)]
mod tests;

pub(super) fn verify_asset_attestation(
    value: &Value,
    name: &str,
    digest: &str,
    source_revision: &str,
) -> Result<(), ProviderFailure> {
    let results = value.as_array().ok_or(ProviderFailure::Attestation)?;
    if results.len() != 1 {
        return Err(ProviderFailure::Attestation);
    }
    let result = &results[0];
    let certificate = result
        .pointer("/verificationResult/signature/certificate")
        .ok_or(ProviderFailure::Attestation)?;
    let statement = result
        .pointer("/verificationResult/statement")
        .ok_or(ProviderFailure::Attestation)?;
    let subject = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::Attestation)?;
    if certificate
        .get("subjectAlternativeName")
        .and_then(Value::as_str)
        != Some(SIGNER_IDENTITY)
        || certificate.get("issuer").and_then(Value::as_str) != Some(OIDC_ISSUER)
        || certificate
            .get("sourceRepositoryURI")
            .and_then(Value::as_str)
            != Some("https://github.com/tailrocks/velnor-new")
        || certificate
            .get("sourceRepositoryDigest")
            .and_then(Value::as_str)
            != Some(source_revision)
        || certificate
            .get("sourceRepositoryRef")
            .and_then(Value::as_str)
            != Some(SOURCE_REF)
        || certificate.get("buildSignerURI").and_then(Value::as_str) != Some(SIGNER_IDENTITY)
        || certificate.get("buildSignerDigest").and_then(Value::as_str) != Some(source_revision)
        || certificate
            .get("githubWorkflowRepository")
            .and_then(Value::as_str)
            != Some(SOURCE_REPOSITORY)
        || certificate.get("githubWorkflowRef").and_then(Value::as_str) != Some(SOURCE_REF)
        || certificate.get("githubWorkflowSHA").and_then(Value::as_str) != Some(source_revision)
        || statement.get("predicateType").and_then(Value::as_str)
            != Some("https://slsa.dev/provenance/v1")
        || !has_exact_target_subject(subject, name, digest)
    {
        return Err(ProviderFailure::Attestation);
    }
    Ok(())
}

fn has_exact_target_subject(subjects: &[Value], name: &str, digest: &str) -> bool {
    let mut names = HashSet::with_capacity(subjects.len());
    let mut target_seen = false;
    for subject in subjects {
        let Some(subject_name) = subject.get("name").and_then(Value::as_str) else {
            return false;
        };
        let Some(subject_digest) = subject.pointer("/digest/sha256").and_then(Value::as_str) else {
            return false;
        };
        if subject_name.is_empty()
            || !valid_subject_digest(subject_digest)
            || !names.insert(subject_name)
        {
            return false;
        }
        if subject_name == name {
            if subject_digest != digest {
                return false;
            }
            target_seen = true;
        }
    }
    target_seen
}

fn valid_subject_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
