// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Record binding (c) and authority-as-trust-root (d).
//!
//! The thread's record demand is answered by the strongest binding the repo
//! actually provides: Sigstore attestations over the exact candidate bytes,
//! verified with `gh attestation verify` and checked with the materialized
//! `validate_attestation_output`. The record itself carries no signature field
//! (grep proof below), so no signature is invented. The fixed authority body
//! stays the sole gate before attestation and publication; no generic final
//! fan-in may interpose (`final_steps.rs` is owned by another agent, so the
//! required removals are documented, not edited).

use super::*;

fn check_record_has_no_signature_field() -> Result<(), Box<dyn Error>> {
    require(RECORD_SOURCE, "RECORD_KEYS = {", "record keys")?;
    forbid(RECORD_SOURCE, "signature", "record signature field")?;
    Ok(())
}

fn check_attestation_binding() -> Result<(), Box<dyn Error>> {
    let text = candidate_text()?;
    require(
        &text,
        "gh attestation verify \\\"candidate/job-image.oci.tar\\\"",
        "archive attestation verification",
    )?;
    require(
        &text,
        "gh attestation verify \\\"candidate/job-image-candidate.json\\\"",
        "record attestation verification",
    )?;
    require(
        &text,
        "Verify candidate attestations with Sigstore",
        "attestation step",
    )?;
    Ok(())
}

fn check_authority_is_trust_root() -> Result<(), Box<dyn Error>> {
    let text = candidate_text()?;
    require_order(
        &text,
        VERIFY_STEP_NAME,
        "Attest all candidate bytes.",
        "verify before attest",
    )?;
    require_order(
        &text,
        "Materialize fixed image authority implementation",
        VERIFY_STEP_NAME,
        "materialize before verify",
    )?;
    require(&text, "attest-image-candidate", "attest job")?;
    require(&text, "publish-job-image", "publish job")?;
    require_order(
        &text,
        "publish_module.publish_image",
        "publish_module.build_evidence",
        "publish before evidence",
    )?;
    require_order(
        &text,
        "fetch_registry_token",
        "publish_module.publish_image",
        "token before publication",
    )?;
    Ok(())
}

fn check_no_final_fanin() -> Result<(), Box<dyn Error>> {
    let text = candidate_text()?;
    for (needle, label) in [
        (
            "Download every expected matrix artifact",
            "generic fetch reports",
        ),
        ("Publish final report", "generic final report"),
        (
            "Download candidate attestation",
            "generic attestation download",
        ),
    ] {
        forbid(&text, needle, label)?;
    }
    Ok(())
}

#[test]
fn gc1_t51_binds_record_without_invented_signature() -> Result<(), Box<dyn Error>> {
    check_record_has_no_signature_field()?;
    check_attestation_binding()?;
    Ok(())
}

#[test]
fn gc1_t51_keeps_authority_as_trust_root() -> Result<(), Box<dyn Error>> {
    check_authority_is_trust_root()?;
    check_no_final_fanin()?;
    Ok(())
}
