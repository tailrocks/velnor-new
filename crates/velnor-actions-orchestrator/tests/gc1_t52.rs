// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_orchestrator::cache_publication::{
    PUBLICATION_SCHEMA, PublicationOutcome, PublicationV1, ReadbackOutcome, SaveHalt, SaveOutcome,
    SaveRequest, classify_publication, decide_save, parse_publication_bytes,
    publication_canonical_bytes, publication_digest,
};
use velnor_actions_orchestrator::cache_recovery::{
    ObservedEntry, READBACK_KEY_MISMATCH, READBACK_RESTORED_DIGEST_MISMATCH, ReadbackInputs,
    ReadbackVerdict, decide_readback, require_empty_restore_target, retain_usable_families,
    sha256_hex, validate_sha256_hex,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_job() -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: "writer".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-t52-plan"))?,
    })
}

fn fixture_generation(tag: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t52-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t52-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(tag))?,
        repair_revision: 0,
    })
}

fn fixture_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-t52-frozen-candidate-marker:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn fixture_entry(generation: &GenerationId, size: u64) -> ObservedEntry {
    ObservedEntry {
        repository_id: 1,
        cache_id: 7,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    }
}

fn measured_bytes(value: u64) -> MeasuredU64 {
    MeasuredU64 {
        value: Some(value),
        unknown_reason: None,
    }
}

fn unknown_bytes() -> MeasuredU64 {
    MeasuredU64 {
        value: None,
        unknown_reason: Some(ObservationGapV1 {
            code: ObservationGapCode::MissingSession,
            detail: "gc1-t52-unobserved-stored-bytes".to_owned(),
            affected_recipe_ids: Vec::new(),
            evidence_ref: "actions-cache-inventory".to_owned(),
        }),
    }
}

fn fixture_save() -> SaveRequest {
    SaveRequest {
        admission_admitted: true,
        candidate_complete: true,
        exact_usable_exists: false,
        denied_reason: None,
        candidate_bytes: Some(1024),
        ceiling_bytes: 8192,
        save_halt: SaveHalt::Running,
    }
}

fn fixture_verified() -> Result<PublicationV1, Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-t52-coverage")?;
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| fail("fixture length out of range"))?;
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: fixture_job()?,
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-t52-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-t52-proof"),
        admission_digest: digest_b3_typed(b"gc1-t52-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(fixture_entry(&generation, size)),
        readback_outcome: ReadbackOutcome::Verified,
        readback_archive_digest: Some(digest),
        readback_manifest_digest: Some(digest_b3_typed(b"gc1-t52-manifest")),
        actual_stored_bytes: measured_bytes(size),
        active_intent_digest: digest_b3_typed(b"gc1-t52-intent"),
    })
}

fn fixture_idle() -> Result<PublicationV1, Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-t52-coverage")?;
    let digest = sha256_hex(&fixture_bytes());
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: fixture_job()?,
        generation,
        frozen_archive_digest: digest,
        manifest_digest: digest_b3_typed(b"gc1-t52-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-t52-proof"),
        admission_digest: digest_b3_typed(b"gc1-t52-admission"),
        save_step_outcome: Some("not-run".to_owned()),
        save_outcome: SaveOutcome::Denied,
        observed_entry: None,
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: unknown_bytes(),
        active_intent_digest: digest_b3_typed(b"gc1-t52-intent"),
    })
}

fn bound_inputs<'a>(
    key: &'a str,
    digest: &'a str,
    size: u64,
    restored: Option<&'a [u8]>,
    entry: Option<&'a ObservedEntry>,
    complete: bool,
) -> ReadbackInputs<'a> {
    ReadbackInputs {
        frozen_archive_digest: digest,
        frozen_archive_bytes: size,
        restored_bytes: restored,
        inventory_complete: complete,
        observed_entry: entry,
        expected_key: key,
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    }
}

fn check_save_caller_path() -> Result<(), Box<dyn Error>> {
    if decide_save(&fixture_save())? != SaveOutcome::Attempted {
        return Err(fail("missing complete admitted generation missed its save"));
    }
    let mut retained = fixture_save();
    retained.exact_usable_exists = true;
    if decide_save(&retained)? != SaveOutcome::SkippedExisting {
        return Err(fail("usable immutable key was not retained unchanged"));
    }
    let mut oversized = fixture_save();
    oversized.candidate_bytes = Some(65536);
    if decide_save(&oversized)? != SaveOutcome::Oversized {
        return Err(fail("oversized candidate escaped the ceiling"));
    }
    Ok(())
}

fn check_publish_caller_path() -> Result<(), Box<dyn Error>> {
    let record = fixture_verified()?;
    let canonical = publication_canonical_bytes(&record)?;
    let parsed = parse_publication_bytes(&canonical)?;
    if parsed != record {
        return Err(fail(
            "verified publication lost its record through the bound path",
        ));
    }
    if publication_digest(&record)? != publication_digest(&parsed)? {
        return Err(fail("publication digest drifted across the bound path"));
    }
    if classify_publication(&record)? != PublicationOutcome::Verified {
        return Err(fail("verified publication missed its verified disposition"));
    }
    Ok(())
}

fn check_restore_caller_path() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-t52-coverage")?;
    let key = generation.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| fail("fixture length out of range"))?;
    let entry = fixture_entry(&generation, size);
    require_empty_restore_target(false)?;
    match decide_readback(&bound_inputs(
        &key,
        &digest,
        size,
        Some(&bytes),
        Some(&entry),
        true,
    ))? {
        ReadbackVerdict::Verified {
            archive_digest,
            stored_bytes,
        } => {
            if archive_digest != digest || stored_bytes != size {
                return Err(fail("verified readback lost its bound evidence"));
            }
        }
        _ => return Err(fail("exact restored bytes missed their verified verdict")),
    }
    Ok(())
}

fn check_retain_caller_path() -> Result<(), Box<dyn Error>> {
    let good = fixture_generation(b"gc1-t52-good")?.cache_key();
    let bad = fixture_generation(b"gc1-t52-bad")?.cache_key();
    let retained = retain_usable_families(&[good.clone(), bad.clone()], &bad)?;
    if retained != vec![good] {
        return Err(fail("usable family did not survive the failed family"));
    }
    Ok(())
}

fn check_incomplete_stays_unverified() -> Result<(), Box<dyn Error>> {
    let idle = fixture_idle()?;
    if classify_publication(&idle)? != PublicationOutcome::Denied {
        return Err(fail("idle publication escaped its denied disposition"));
    }
    let mut unavailable = fixture_verified()?;
    unavailable.readback_outcome = ReadbackOutcome::Unavailable;
    unavailable.readback_archive_digest = None;
    unavailable.readback_manifest_digest = None;
    unavailable.actual_stored_bytes = unknown_bytes();
    if classify_publication(&unavailable)? != PublicationOutcome::Interrupted {
        return Err(fail("unavailable readback presented as verified"));
    }
    Ok(())
}

#[test]
fn gc1_t52_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_save_caller_path()?;
    check_publish_caller_path()?;
    check_restore_caller_path()?;
    check_retain_caller_path()?;
    check_incomplete_stays_unverified()?;
    Ok(())
}

fn check_rejects_bad_save() -> Result<(), Box<dyn Error>> {
    let mut ceiling = fixture_save();
    ceiling.ceiling_bytes = 0;
    if decide_save(&ceiling).is_ok() {
        return Err(fail("zero ceiling validated"));
    }
    let mut unexplained = fixture_save();
    unexplained.admission_admitted = false;
    if decide_save(&unexplained).is_ok() {
        return Err(fail("denial without reason validated"));
    }
    let mut blank_reason = fixture_save();
    blank_reason.admission_admitted = false;
    blank_reason.denied_reason = Some(String::new());
    if decide_save(&blank_reason).is_ok() {
        return Err(fail("blank denial reason validated"));
    }
    Ok(())
}

fn check_rejects_bad_readback() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-t52-coverage")?;
    let key = generation.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| fail("fixture length out of range"))?;
    let entry = fixture_entry(&generation, size);
    let other = fixture_generation(b"gc1-t52-other")?.cache_key();
    match decide_readback(&bound_inputs(
        &other,
        &digest,
        size,
        Some(&bytes),
        Some(&entry),
        true,
    ))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_KEY_MISMATCH {
                return Err(fail("key mismatch lost its evidence"));
            }
        }
        _ => return Err(fail("foreign key readback escaped corrupt")),
    }
    let mut tampered = bytes.clone();
    tampered[0] ^= 0xff;
    match decide_readback(&bound_inputs(
        &key,
        &digest,
        size,
        Some(&tampered),
        Some(&entry),
        true,
    ))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_RESTORED_DIGEST_MISMATCH {
                return Err(fail("digest mismatch lost its evidence"));
            }
        }
        _ => return Err(fail("tampered bytes escaped corrupt")),
    }
    if decide_readback(&bound_inputs(
        &key,
        "not-a-digest",
        size,
        Some(&bytes),
        Some(&entry),
        true,
    ))
    .is_ok()
    {
        return Err(fail("malformed frozen digest validated"));
    }
    if validate_sha256_hex(&digest.to_uppercase()).is_ok() {
        return Err(fail("uppercase digest spelling validated"));
    }
    if require_empty_restore_target(true).is_ok() {
        return Err(fail("occupied restore target validated"));
    }
    if retain_usable_families(&[key], "not-a-generation-key").is_ok() {
        return Err(fail("malformed failed key validated"));
    }
    Ok(())
}

fn check_rejects_bad_publication_record() -> Result<(), Box<dyn Error>> {
    let mut record = fixture_verified()?;
    record.readback_archive_digest = Some(sha256_hex(b"gc1-t52-other-bytes"));
    if publication_canonical_bytes(&record).is_ok() {
        return Err(fail("verified record with mismatched archive validated"));
    }
    let canonical = publication_canonical_bytes(&fixture_verified()?)?;
    let mut value: serde_json::Value = serde_json::from_slice(&canonical)?;
    value["schema"] = serde_json::Value::from(2);
    let downgraded = serde_json::to_vec(&value)?;
    if parse_publication_bytes(&downgraded).is_ok() {
        return Err(fail("unsupported publication schema validated"));
    }
    let mut unknown = serde_json::from_slice::<serde_json::Value>(&canonical)?;
    unknown["unknown_field"] = serde_json::Value::from(true);
    let extended = serde_json::to_vec(&unknown)?;
    if parse_publication_bytes(&extended).is_ok() {
        return Err(fail("publication record with unknown field validated"));
    }
    Ok(())
}

#[test]
fn gc1_t52_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_save()?;
    check_rejects_bad_readback()?;
    check_rejects_bad_publication_record()?;
    Ok(())
}
