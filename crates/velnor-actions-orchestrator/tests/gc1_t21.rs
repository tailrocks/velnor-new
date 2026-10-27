// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use velnor_actions_orchestrator::{
    ENVELOPE_SCHEMA, FrozenInnerPayload, HARD_CEILING_BYTES, MAX_GENERATION_SLOTS,
    MIN_RESERVE_BYTES, MeasuredEnvelope, OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME, OuterEnvelope,
    OuterEnvelopeIdentity, PackedArchive, UNKNOWN_HOSTED_PACKAGING, UNKNOWN_REPACK_BASIS,
    UNKNOWN_RUNNER_LAYOUT, UNKNOWN_TOOL_IDENTITY, envelope_canonical_bytes, freeze_inner_payload,
    measure_envelope, parse_envelope_bytes, run_preflight_packaging, sha256_hex, unknown_envelope,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_inner(root: &Path, name: &str, seed: &[u8]) -> Result<PackedArchive, Box<dyn Error>> {
    let path = root.join(name);
    let mut bytes = Vec::new();
    for round in 0_u32..64 {
        bytes.extend_from_slice(seed);
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    fs::write(&path, &bytes)?;
    Ok(PackedArchive {
        archive_sha256: sha256_hex(&bytes),
        archive_bytes: u64::try_from(bytes.len())?,
        member_count: 7,
    })
}

fn fixture_identity() -> OuterEnvelopeIdentity {
    OuterEnvelopeIdentity {
        tool_identity: "tar-0.4.46+flate2-1.1.10".to_owned(),
        transport_member: OUTER_MEMBER_NAME.to_owned(),
    }
}

fn measured_fixture(
    root: &Path,
    hosted: bool,
) -> Result<(FrozenInnerPayload, MeasuredEnvelope), Box<dyn Error>> {
    let packed = fixture_inner(root, "payload.tar.gz", b"gc1-t21-inner")?;
    let frozen = freeze_inner_payload(&packed)?;
    if frozen.archive_bytes != packed.archive_bytes
        || frozen.archive_sha256 != packed.archive_sha256
    {
        return Err(fail("frozen inner drifted from packed bytes"));
    }
    let preflight =
        run_preflight_packaging(&root.join("payload.tar.gz"), &root.join("outer.tar.gz"))?;
    let second =
        run_preflight_packaging(&root.join("payload.tar.gz"), &root.join("outer-2.tar.gz"))?;
    if preflight != second {
        return Err(fail("preflight packaging drifted"));
    }
    if preflight.measured_outer_bytes == 0 || preflight.outer_sha256.len() != 64 {
        return Err(fail("preflight measurement missing"));
    }
    let allowance = 1_048_576_u64;
    let measured = measure_envelope(&frozen, &preflight, allowance, &fixture_identity(), hosted)?;
    if measured.candidate_upper_bound_bytes != preflight.measured_outer_bytes + allowance {
        return Err(fail("upper bound drifted from measured plus allowance"));
    }
    Ok((frozen, measured))
}

#[test]
fn gc1_t21_preserves_contract() -> Result<(), Box<dyn Error>> {
    if HARD_CEILING_BYTES != 10_000_000_000
        || OPERATING_CEILING_BYTES != 8_000_000_000
        || MIN_RESERVE_BYTES != 2_000_000_000
        || HARD_CEILING_BYTES - OPERATING_CEILING_BYTES != MIN_RESERVE_BYTES
    {
        return Err(fail("accounting ceiling drifted"));
    }
    if MAX_GENERATION_SLOTS != 2 || ENVELOPE_SCHEMA != 1 {
        return Err(fail("envelope policy drifted"));
    }
    let temp = tempfile::tempdir()?;
    let (_frozen, measured) = measured_fixture(temp.path(), false)?;
    if measured.hosted_measurement {
        return Err(fail("local fixture claimed hosted measurement"));
    }
    let envelope = OuterEnvelope::Measured(measured.clone());
    if !envelope.is_measured() || envelope.candidate_upper_bound_bytes().is_none() {
        return Err(fail("measured envelope lost its bound"));
    }
    if envelope.qualifies_for_publication() {
        return Err(fail("local envelope qualified for publication"));
    }
    if measured.candidate_upper_bound_bytes >= HARD_CEILING_BYTES {
        return Err(fail("fixture bound exceeded hard ceiling"));
    }
    let hosted = measure_envelope(
        &measured.inner,
        &velnor_actions_orchestrator::PreflightMeasurement {
            inner: measured.inner.clone(),
            measured_outer_bytes: measured.measured_outer_bytes,
            outer_sha256: measured.outer_sha256.clone(),
        },
        measured.repack_allowance_bytes,
        &fixture_identity(),
        true,
    )?;
    if !OuterEnvelope::Measured(hosted).qualifies_for_publication() {
        return Err(fail("hosted envelope missed publication qualification"));
    }
    let bytes = envelope_canonical_bytes(&envelope)?;
    let round_trip = parse_envelope_bytes(&bytes)?;
    if round_trip != envelope {
        return Err(fail("envelope strict round trip drifted"));
    }
    let unknown = OuterEnvelope::Unknown(unknown_envelope(
        UNKNOWN_HOSTED_PACKAGING,
        "hosted runner packaging not yet measured",
    )?);
    if unknown.is_measured()
        || unknown.candidate_upper_bound_bytes().is_some()
        || unknown.qualifies_for_publication()
    {
        return Err(fail("unknown envelope claimed a bound"));
    }
    let unknown_bytes = envelope_canonical_bytes(&unknown)?;
    if parse_envelope_bytes(&unknown_bytes)? != unknown {
        return Err(fail("unknown envelope strict round trip drifted"));
    }
    Ok(())
}

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

fn reject_bad_freeze() -> Result<(), Box<dyn Error>> {
    must_fail!(
        freeze_inner_payload(&PackedArchive {
            archive_sha256: sha256_hex(b"gc1-t21-bytes"),
            archive_bytes: 0,
            member_count: 1,
        }),
        "empty inner",
    );
    must_fail!(
        freeze_inner_payload(&PackedArchive {
            archive_sha256: "not-a-digest".to_owned(),
            archive_bytes: 8,
            member_count: 1,
        }),
        "inner digest",
    );
    Ok(())
}

fn reject_bad_measure(
    inner: &FrozenInnerPayload,
    measured: &MeasuredEnvelope,
) -> Result<(), Box<dyn Error>> {
    let preflight = velnor_actions_orchestrator::PreflightMeasurement {
        inner: (*inner).clone(),
        measured_outer_bytes: measured.measured_outer_bytes,
        outer_sha256: measured.outer_sha256.clone(),
    };
    must_fail!(
        measure_envelope(inner, &preflight, u64::MAX, &fixture_identity(), true),
        "bound overflow",
    );
    let mut drifted = measured.clone();
    drifted.candidate_upper_bound_bytes += 1;
    must_fail!(
        envelope_canonical_bytes(&OuterEnvelope::Measured(drifted)),
        "bound mismatch",
    );
    let mut wrong_schema = measured.clone();
    wrong_schema.schema = 2;
    must_fail!(
        envelope_canonical_bytes(&OuterEnvelope::Measured(wrong_schema)),
        "measured schema",
    );
    let mut wrong_member = fixture_identity();
    "other.tar.gz".clone_into(&mut wrong_member.transport_member);
    must_fail!(
        measure_envelope(inner, &preflight, 0, &wrong_member, false),
        "transport member",
    );
    let mut empty_tool = fixture_identity();
    empty_tool.tool_identity.clear();
    must_fail!(
        measure_envelope(inner, &preflight, 0, &empty_tool, false),
        "tool identity",
    );
    let mut bad_outer = measured.clone();
    "zz".clone_into(&mut bad_outer.outer_sha256);
    must_fail!(
        envelope_canonical_bytes(&OuterEnvelope::Measured(bad_outer)),
        "outer digest",
    );
    Ok(())
}

fn reject_bad_unknown() -> Result<(), Box<dyn Error>> {
    must_fail!(
        unknown_envelope("unknown-made-up-code", "detail"),
        "unknown code"
    );
    must_fail!(unknown_envelope(UNKNOWN_TOOL_IDENTITY, ""), "empty detail");
    must_fail!(
        unknown_envelope(UNKNOWN_REPACK_BASIS, "has\0nul"),
        "nul detail",
    );
    let mut schema = unknown_envelope(UNKNOWN_RUNNER_LAYOUT, "layout pending")?;
    schema.schema = 2;
    must_fail!(
        envelope_canonical_bytes(&OuterEnvelope::Unknown(schema)),
        "unknown schema",
    );
    Ok(())
}

fn reject_bad_preflight(root: &Path) -> Result<(), Box<dyn Error>> {
    must_fail!(
        run_preflight_packaging(&root.join("absent.tar.gz"), &root.join("out.tar.gz")),
        "missing inner",
    );
    let packed = fixture_inner(root, "payload.tar.gz", b"gc1-t21-preflight")?;
    let _ = packed;
    let taken = root.join("taken.tar.gz");
    fs::write(&taken, b"existing")?;
    must_fail!(
        run_preflight_packaging(&root.join("payload.tar.gz"), &taken),
        "existing preflight output",
    );
    #[cfg(unix)]
    {
        let link = root.join("link.tar.gz");
        std::os::unix::fs::symlink("payload.tar.gz", &link)?;
        must_fail!(
            run_preflight_packaging(&link, &root.join("link-out.tar.gz")),
            "symlink inner",
        );
    }
    must_fail!(
        run_preflight_packaging(root, &root.join("dir-out.tar.gz")),
        "directory inner",
    );
    Ok(())
}

fn reject_bad_envelope_bytes(envelope: &OuterEnvelope) -> Result<(), Box<dyn Error>> {
    let bytes = envelope_canonical_bytes(envelope)?;
    must_fail!(
        parse_envelope_bytes(&bytes[..bytes.len() / 2]),
        "truncated envelope",
    );
    must_fail!(
        parse_envelope_bytes(br#"{"status":"measured","status":"measured"}"#),
        "duplicate key",
    );
    let mut value = serde_json::to_value(envelope)?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("envelope not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_envelope_bytes(&serde_json::to_vec(&value)?),
        "unknown field",
    );
    let mut status = serde_json::to_value(envelope)?;
    status
        .as_object_mut()
        .ok_or_else(|| fail("envelope not object"))?
        .insert("status".to_owned(), serde_json::Value::from("made-up"));
    must_fail!(
        parse_envelope_bytes(&serde_json::to_vec(&status)?),
        "unknown status",
    );
    Ok(())
}

#[test]
fn gc1_t21_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    reject_bad_freeze()?;
    reject_bad_unknown()?;
    let temp = tempfile::tempdir()?;
    reject_bad_preflight(temp.path())?;
    let packed = fixture_inner(temp.path(), "inner.tar.gz", b"gc1-t21-reject")?;
    let inner = freeze_inner_payload(&packed)?;
    let preflight = run_preflight_packaging(
        &temp.path().join("inner.tar.gz"),
        &temp.path().join("inner-outer.tar.gz"),
    )?;
    let measured = measure_envelope(&inner, &preflight, 0, &fixture_identity(), false)?;
    reject_bad_measure(&inner, &measured)?;
    let envelope = OuterEnvelope::Measured(measured);
    reject_bad_envelope_bytes(&envelope)?;
    let unknown = OuterEnvelope::Unknown(unknown_envelope(
        UNKNOWN_TOOL_IDENTITY,
        "pinned tool behavior not yet qualified",
    )?);
    reject_bad_envelope_bytes(&unknown)?;
    if unknown.qualifies_for_publication() || unknown.candidate_upper_bound_bytes().is_some() {
        return Err(fail("unknown envelope qualified"));
    }
    Ok(())
}
