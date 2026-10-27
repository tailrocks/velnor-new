// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{
    EntryKind, PackedArchive, extract_snapshot_archive, freeze_inner_payload, measure_envelope,
    run_preflight_packaging, sha256_hex,
};

use super::{build_tiny_fixture, expect_code, fail, projection_records, repack_plan};

#[test]
fn review_projection_symlink_hash_drift_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = build_tiny_fixture("payload.tar.gz", 0o644)?;
    let mut projection = projection_records(&fixture.plan);
    let record = projection
        .iter_mut()
        .find(|record| record.kind == EntryKind::Symlink)
        .ok_or_else(|| fail("tiny plan must contain a projection symlink"))?;
    record.content_sha256 = sha256_hex(b"drifted-target-bytes");
    let tampered = repack_plan(
        &fixture,
        &fixture.plan.state_index,
        &projection,
        "drift.tar.gz",
    )?;
    let destination = fixture.temp.path().join("restore-drift");
    let error = extract_snapshot_archive(&tampered, &destination)
        .expect_err("projection symlink hash drift must be rejected");
    expect_code(&error, "snapshot_archive_link_mismatch")?;
    Ok(())
}

#[test]
fn review_state_special_mode_bits_rejected_at_extract() -> Result<(), Box<dyn Error>> {
    let fixture = build_tiny_fixture("payload.tar.gz", 0o644)?;
    let mut state = fixture.plan.state_index.clone();
    let entry = state
        .iter_mut()
        .find(|entry| entry.link_target.is_none())
        .ok_or_else(|| fail("tiny plan must contain a regular state entry"))?;
    entry.mode |= 0o4000;
    let tampered = repack_plan(
        &fixture,
        &state,
        &projection_records(&fixture.plan),
        "setuid.tar.gz",
    )?;
    let destination = fixture.temp.path().join("restore-setuid");
    let error = extract_snapshot_archive(&tampered, &destination)
        .expect_err("special mode bits must be rejected at extract time");
    expect_code(&error, "snapshot_mode_special_bits")?;
    Ok(())
}

#[test]
fn review_stale_preflight_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let inner_a = temp.path().join("inner-a.tar.gz");
    let inner_b = temp.path().join("inner-b.tar.gz");
    fs::write(&inner_a, b"payload-a")?;
    fs::write(&inner_b, b"payload-b-unrelated")?;
    let frozen_a = freeze_inner_payload(&PackedArchive {
        archive_sha256: sha256_hex(b"payload-a"),
        archive_bytes: 9,
        member_count: 1,
    })?;
    let preflight_b = run_preflight_packaging(&inner_b, &temp.path().join("outer-b.tar.gz"))?;
    let error = measure_envelope(
        &frozen_a,
        &preflight_b,
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "review-threads-packer".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )
    .expect_err("stale preflight must not qualify publication");
    expect_code(&error, "envelope_preflight_inner_mismatch")?;
    let preflight_a = run_preflight_packaging(&inner_a, &temp.path().join("outer-a.tar.gz"))?;
    let measured = measure_envelope(
        &frozen_a,
        &preflight_a,
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "review-threads-packer".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )?;
    if !velnor_actions_orchestrator::OuterEnvelope::Measured(measured).qualifies_for_publication() {
        return Err(fail("matching preflight must still qualify"));
    }
    Ok(())
}
