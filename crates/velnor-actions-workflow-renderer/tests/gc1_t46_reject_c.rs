// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_manifest_reads(
    fixture: &Fixture,
    archive_json: &str,
    manifest: &[u8],
    manifest_digest: &str,
    data: &[u8],
    data_hex: &str,
) -> Result<(), Box<dyn Error>> {
    let envelope = format!(
        "{{\"archive\":{archive},\"digest\":\"{digest}\",\"size\":{size}}}",
        archive = archive_json,
        digest = manifest_digest,
        size = manifest.len() + 1,
    );
    expect_rejection(
        fixture,
        "manifest",
        &envelope,
        "manifest-size",
        "MANIFEST-REJECTED",
        Some("OCI platform manifest bytes changed after validation."),
    )?;
    let envelope = format!(
        "{{\"archive\":{archive},\"digest\":\"sha256:{missing}\",\"size\":{size}}}",
        archive = archive_json,
        missing = "d".repeat(64),
        size = data.len(),
    );
    expect_rejection(
        fixture,
        "manifest",
        &envelope,
        "manifest-missing",
        "MANIFEST-REJECTED",
        Some("Cannot read OCI blob from archive."),
    )?;
    let envelope = format!(
        "{{\"archive\":{archive},\"digest\":\"sha256:{data_hex}\",\"size\":{size}}}",
        archive = archive_json,
        data_hex = data_hex,
        size = data.len() + 1,
    );
    expect_rejection(
        fixture,
        "extract",
        &envelope,
        "extract-size",
        "EXTRACT-REJECTED",
        Some("OCI blob descriptor size is incorrect."),
    )?;
    Ok(())
}

pub(crate) fn expect_evidence_rejection(
    fixture: &Fixture,
    stem: &str,
    mutated: &str,
    message: &str,
) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "evidence_check",
        &format!("{{\"evidence\":{mutated}}}"),
        stem,
        "EVIDENCE-REJECTED",
        Some(message),
    )
}

pub(crate) fn check_rejects_evidence(
    fixture: &Fixture,
    evidence: &str,
) -> Result<(), Box<dyn Error>> {
    expect_evidence_rejection(
        fixture,
        "evidence-schema",
        &replace_once(
            evidence,
            "velnor.job-image-publication.v1",
            "other.schema.v1",
        )?,
        "Publication evidence schema or status is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-status",
        &replace_once(evidence, "\"status\":\"PASS\"", "\"status\":\"FAIL\"")?,
        "Publication evidence schema or status is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-package",
        &replace_once(
            evidence,
            "\"package\":\"ghcr.io/tailrocks/velnor-job\"",
            "\"package\":\"ghcr.io/other/pkg\"",
        )?,
        "Publication evidence package is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-reference",
        &replace_once(evidence, "\"digest_reference\"", "\"digest_reference_old\"")?,
        "Publication evidence has unknown or missing fields.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-source",
        &replace_once(
            evidence,
            SOURCE_SHA,
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        )?,
        "Publication evidence source is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-workflow",
        &replace_once(evidence, "\"run_id\":7", "\"run_id\":0")?,
        "Publication evidence workflow run_id is incorrect.",
    )?;
    check_rejects_evidence_tail(fixture, evidence)?;
    Ok(())
}

pub(crate) fn check_rejects_evidence_tail(
    fixture: &Fixture,
    evidence: &str,
) -> Result<(), Box<dyn Error>> {
    expect_evidence_rejection(
        fixture,
        "evidence-ci",
        &replace_once(
            evidence,
            "\"run_attempt\":1,\"run_id\":11",
            "\"run_attempt\":1,\"run_id\":\"11\"",
        )?,
        "Publication evidence CI identity is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-layers",
        &replace_once(
            evidence,
            "\"layer_digests\":[",
            "\"layer_digests\":[],\"x\":[",
        )?,
        "Publication evidence layer digests are incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-gate",
        &replace_once(evidence, "\"not_proven\"", "\"proven\"")?,
        "Publication evidence access gate is incorrect.",
    )?;
    expect_evidence_rejection(
        fixture,
        "evidence-record",
        &replace_once(evidence, "\"record_sha256\"", "\"record_sha256_old\"")?,
        "Publication evidence has unknown or missing fields.",
    )?;
    Ok(())
}

#[test]
fn gc1_t46_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_sources_are_fixed()?;
    let fixture = fixture()?;
    let identity = identity_envelope();
    check_rejects_identity(&fixture, &identity)?;
    check_rejects_authorization(&fixture)?;
    check_rejects_ci_and_attestation(&fixture)?;
    check_rejects_source_and_lock(&fixture)?;
    check_rejects_registry_locations(&fixture)?;
    check_rejects_registry_auth(&fixture)?;
    check_rejects_retrieval(&fixture)?;
    check_rejects_publication(&fixture)?;
    let evidence = check_accepts_evidence(&fixture)?;
    check_rejects_evidence(&fixture, &evidence)?;
    check_rejects_mismatched_source(&fixture)?;
    Ok(())
}
