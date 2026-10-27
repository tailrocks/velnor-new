// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

#[expect(
    unreachable_pub,
    reason = "T26 publication module is private to the crate; the test exercises it by path include"
)]
#[path = "../src/cache_publication.rs"]
mod cache_publication;

#[expect(
    unreachable_pub,
    reason = "T26 recovery module is private to the crate; the test exercises it by path include"
)]
#[path = "../src/cache_recovery.rs"]
mod cache_recovery;

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use self::cache_publication::{
    MAX_PUBLICATION_JSON_BYTES, MAX_PUBLICATION_TEXT_BYTES, PUBLICATION_SCHEMA, PublicationV1,
    ReadbackOutcome, SaveHalt, SaveOutcome, SaveRequest, classify_publication, decide_save,
    parse_publication_bytes, publication_canonical_bytes, publication_digest,
};
use self::cache_recovery::{
    MAX_RECOVERY_TEXT_BYTES, ObservedEntry, READBACK_ENTRY_NOT_OBSERVED, READBACK_KEY_MISMATCH,
    READBACK_NOT_AVAILABLE, READBACK_REF_MISMATCH, READBACK_RESTORED_DIGEST_MISMATCH,
    READBACK_RESTORED_SIZE_MISMATCH, READBACK_STORED_BYTES_MISMATCH, READBACK_UNKNOWN_INVENTORY,
    READBACK_VERSION_MISMATCH, ReadbackInputs, ReadbackVerdict, decide_readback,
    require_empty_restore_target, retain_usable_families, sha256_hex, validate_sha256_hex,
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
        plan_digest: Digest::parse(&digest_b3(b"gc1-t26-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t26-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t26-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t26-coverage"))?,
        repair_revision: 0,
    })
}

fn fixture_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-t26-frozen-candidate-marker:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn fixture_size() -> Result<u64, Box<dyn Error>> {
    u64::try_from(fixture_bytes().len()).map_err(|_| fail("fixture length out of range"))
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
            detail: "gc1-t26-unobserved-stored-bytes".to_owned(),
            affected_recipe_ids: Vec::new(),
            evidence_ref: "actions-cache-inventory".to_owned(),
        }),
    }
}

fn fixture_verified() -> Result<PublicationV1, Box<dyn Error>> {
    let generation = fixture_generation()?;
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = fixture_size()?;
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: fixture_job()?,
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-t26-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-t26-proof"),
        admission_digest: digest_b3_typed(b"gc1-t26-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(fixture_entry(&generation, size)),
        readback_outcome: ReadbackOutcome::Verified,
        readback_archive_digest: Some(digest),
        readback_manifest_digest: Some(digest_b3_typed(b"gc1-t26-manifest")),
        actual_stored_bytes: measured_bytes(size),
        active_intent_digest: digest_b3_typed(b"gc1-t26-intent"),
    })
}

fn fixture_idle(save_outcome: SaveOutcome) -> Result<PublicationV1, Box<dyn Error>> {
    let generation = fixture_generation()?;
    let digest = sha256_hex(&fixture_bytes());
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: fixture_job()?,
        generation,
        frozen_archive_digest: digest,
        manifest_digest: digest_b3_typed(b"gc1-t26-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-t26-proof"),
        admission_digest: digest_b3_typed(b"gc1-t26-admission"),
        save_step_outcome: Some("not-run".to_owned()),
        save_outcome,
        observed_entry: None,
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: unknown_bytes(),
        active_intent_digest: digest_b3_typed(b"gc1-t26-intent"),
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

fn check_save_only_missing_complete_admitted() -> Result<(), Box<dyn Error>> {
    if decide_save(&fixture_save())? != SaveOutcome::Attempted {
        return Err(fail("missing complete admitted generation missed its save"));
    }
    let mut usable = fixture_save();
    usable.exact_usable_exists = true;
    if decide_save(&usable)? != SaveOutcome::SkippedExisting {
        return Err(fail("usable immutable key was not retained unchanged"));
    }
    let mut denied = fixture_save();
    denied.denied_reason = Some("policy-denied".to_owned());
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(fail("explicit denial missed its outcome"));
    }
    let mut blocked = fixture_save();
    blocked.admission_admitted = false;
    blocked.denied_reason = Some("unknown-inventory".to_owned());
    if decide_save(&blocked)? != SaveOutcome::Denied {
        return Err(fail("blocked admission missed its denial"));
    }
    let mut incomplete = fixture_save();
    incomplete.candidate_complete = false;
    if decide_save(&incomplete)? != SaveOutcome::Denied {
        return Err(fail("incomplete candidate escaped its denial"));
    }
    let mut oversized = fixture_save();
    oversized.candidate_bytes = Some(1_000_000);
    if decide_save(&oversized)? != SaveOutcome::Oversized {
        return Err(fail("oversized candidate missed its outcome"));
    }
    let mut unknown = fixture_save();
    unknown.candidate_bytes = None;
    unknown.ceiling_bytes = 1;
    if decide_save(&unknown)? != SaveOutcome::Attempted {
        return Err(fail("unknown size was zero-filled into oversized"));
    }
    let mut interrupted = fixture_save();
    interrupted.save_halt = SaveHalt::Interrupted;
    if decide_save(&interrupted)? != SaveOutcome::Interrupted {
        return Err(fail("interrupted save missed its outcome"));
    }
    let mut failed = fixture_save();
    failed.save_halt = SaveHalt::ServiceFailed;
    if decide_save(&failed)? != SaveOutcome::ServiceFailure {
        return Err(fail("failed service missed its outcome"));
    }
    let mut denied_usable = fixture_save();
    denied_usable.exact_usable_exists = true;
    denied_usable.denied_reason = Some("policy-denied".to_owned());
    if decide_save(&denied_usable)? != SaveOutcome::Denied {
        return Err(fail("denial lost priority over existing key"));
    }
    let mut incomplete_usable = fixture_save();
    incomplete_usable.exact_usable_exists = true;
    incomplete_usable.candidate_complete = false;
    if decide_save(&incomplete_usable)? != SaveOutcome::SkippedExisting {
        return Err(fail(
            "existing key lost its skip to an incomplete candidate",
        ));
    }
    Ok(())
}

fn check_verified_round_trip() -> Result<(), Box<dyn Error>> {
    if PUBLICATION_SCHEMA != 1 {
        return Err(fail("publication schema drifted"));
    }
    let record = fixture_verified()?;
    record
        .validate()
        .map_err(|error| fail(&error.to_string()))?;
    let bytes = publication_canonical_bytes(&record).map_err(|error| fail(&error.to_string()))?;
    if parse_publication_bytes(&bytes).map_err(|error| fail(&error.to_string()))? != record {
        return Err(fail("publication strict round trip drifted"));
    }
    let again = parse_publication_bytes(&bytes).map_err(|error| fail(&error.to_string()))?;
    if publication_digest(&record).map_err(|error| fail(&error.to_string()))?
        != publication_digest(&again).map_err(|error| fail(&error.to_string()))?
    {
        return Err(fail("publication digest drifted across round trip"));
    }
    if classify_publication(&record).map_err(|error| fail(&error.to_string()))?
        != cache_publication::PublicationOutcome::Verified
    {
        return Err(fail("verified publication missed its outcome"));
    }
    Ok(())
}

fn outcome_of(
    record: &PublicationV1,
) -> Result<cache_publication::PublicationOutcome, Box<dyn Error>> {
    classify_publication(record).map_err(|error| fail(&error.to_string()))
}

fn check_five_outcomes() -> Result<(), Box<dyn Error>> {
    use cache_publication::PublicationOutcome;
    if outcome_of(&fixture_verified()?)? != PublicationOutcome::Verified {
        return Err(fail("verified path missed its outcome"));
    }
    if outcome_of(&fixture_idle(SaveOutcome::Denied)?)? != PublicationOutcome::Denied {
        return Err(fail("denied path missed its outcome"));
    }
    if outcome_of(&fixture_idle(SaveOutcome::Oversized)?)? != PublicationOutcome::Oversized {
        return Err(fail("oversized path missed its outcome"));
    }
    if outcome_of(&fixture_idle(SaveOutcome::Interrupted)?)? != PublicationOutcome::Interrupted {
        return Err(fail("interrupted path missed its outcome"));
    }
    if outcome_of(&fixture_idle(SaveOutcome::ServiceFailure)?)? != PublicationOutcome::Interrupted {
        return Err(fail("service failure missed its interrupted outcome"));
    }
    let mut corrupt = fixture_verified()?;
    corrupt.save_outcome = SaveOutcome::Attempted;
    corrupt.readback_outcome = ReadbackOutcome::Corrupt;
    corrupt.readback_archive_digest = Some("1".repeat(64));
    corrupt.readback_manifest_digest = Some(digest_b3_typed(b"gc1-t26-manifest"));
    if outcome_of(&corrupt)? != PublicationOutcome::Corrupt {
        return Err(fail("corrupt path missed its outcome"));
    }
    let mut unavailable = fixture_verified()?;
    unavailable.save_outcome = SaveOutcome::Attempted;
    unavailable.readback_outcome = ReadbackOutcome::Unavailable;
    unavailable.readback_archive_digest = None;
    unavailable.readback_manifest_digest = None;
    if outcome_of(&unavailable)? != PublicationOutcome::Interrupted {
        return Err(fail("unavailable readback missed its interrupted outcome"));
    }
    let mut skipped = fixture_verified()?;
    skipped.save_outcome = SaveOutcome::SkippedExisting;
    if outcome_of(&skipped)? != PublicationOutcome::Verified {
        return Err(fail("retained existing key missed its verified outcome"));
    }
    Ok(())
}

#[path = "gc1_t26_a.rs"]
mod gc1_t26_a;
#[path = "gc1_t26_b.rs"]
mod gc1_t26_b;
