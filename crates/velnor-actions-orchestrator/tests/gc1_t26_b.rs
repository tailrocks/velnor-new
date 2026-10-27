// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_rejects_bad_save_requests() -> Result<(), Box<dyn Error>> {
    let mut ceiling = fixture_save();
    ceiling.ceiling_bytes = 0;
    must_fail!(decide_save(&ceiling), "zero ceiling");
    let mut blocked = fixture_save();
    blocked.admission_admitted = false;
    must_fail!(decide_save(&blocked), "denial without reason");
    let mut halted_oversized = fixture_save();
    halted_oversized.candidate_bytes = Some(1_000_000);
    halted_oversized.save_halt = SaveHalt::Interrupted;
    if decide_save(&halted_oversized)? != SaveOutcome::Oversized {
        return Err(fail("oversized candidate lost priority over halt"));
    }
    let mut empty_reason = fixture_save();
    empty_reason.denied_reason = Some(String::new());
    must_fail!(decide_save(&empty_reason), "empty denial reason");
    Ok(())
}

fn check_rejects_idle_mismatch() -> Result<(), Box<dyn Error>> {
    let mut readback = fixture_idle(SaveOutcome::Denied)?;
    readback.readback_outcome = ReadbackOutcome::Verified;
    must_fail!(
        publication_canonical_bytes(&readback),
        "readback without save"
    );
    let mut entry = fixture_idle(SaveOutcome::Oversized)?;
    entry.observed_entry = Some(fixture_entry(&fixture_generation()?, 8));
    must_fail!(publication_canonical_bytes(&entry), "entry without save");
    let mut digests = fixture_idle(SaveOutcome::Interrupted)?;
    digests.readback_archive_digest = Some("2".repeat(64));
    must_fail!(
        publication_canonical_bytes(&digests),
        "readback digest without save"
    );
    let mut stored = fixture_idle(SaveOutcome::ServiceFailure)?;
    stored.actual_stored_bytes = measured_bytes(8);
    must_fail!(
        publication_canonical_bytes(&stored),
        "stored bytes without save"
    );
    let mut nosave = fixture_verified()?;
    nosave.save_outcome = SaveOutcome::Attempted;
    nosave.readback_outcome = ReadbackOutcome::NotAttempted;
    must_fail!(
        publication_canonical_bytes(&nosave),
        "save without readback"
    );
    Ok(())
}

fn check_rejects_verified_mismatch() -> Result<(), Box<dyn Error>> {
    let mut schema = fixture_verified()?;
    schema.schema = 2;
    must_fail!(publication_canonical_bytes(&schema), "record schema");
    let mut digest = fixture_verified()?;
    "not-a-digest".clone_into(&mut digest.frozen_archive_digest);
    must_fail!(publication_canonical_bytes(&digest), "frozen digest");
    let mut archive = fixture_verified()?;
    archive.readback_archive_digest = Some("3".repeat(64));
    must_fail!(
        publication_canonical_bytes(&archive),
        "verified archive drift"
    );
    let mut manifest = fixture_verified()?;
    manifest.readback_manifest_digest = Some(digest_b3_typed(b"gc1-t26-other"));
    must_fail!(
        publication_canonical_bytes(&manifest),
        "verified manifest drift"
    );
    let mut unknown = fixture_verified()?;
    unknown.actual_stored_bytes = unknown_bytes();
    must_fail!(
        publication_canonical_bytes(&unknown),
        "verified bytes unknown"
    );
    let mut drifted = fixture_verified()?;
    drifted.actual_stored_bytes = measured_bytes(1);
    must_fail!(
        publication_canonical_bytes(&drifted),
        "verified bytes drift"
    );
    let mut corrupt = fixture_verified()?;
    corrupt.readback_outcome = ReadbackOutcome::Corrupt;
    corrupt.readback_archive_digest = None;
    corrupt.readback_manifest_digest = None;
    must_fail!(
        publication_canonical_bytes(&corrupt),
        "corrupt without evidence"
    );
    let mut clean = fixture_verified()?;
    clean.readback_outcome = ReadbackOutcome::Corrupt;
    must_fail!(
        publication_canonical_bytes(&clean),
        "corrupt without mismatch"
    );
    let mut key = fixture_verified()?;
    let other = GenerationId {
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t26-foreign"))?,
        ..fixture_generation()?
    };
    let size = fixture_size()?;
    key.observed_entry = Some(fixture_entry(&other, size));
    must_fail!(publication_canonical_bytes(&key), "entry key drift");
    let mut repository = fixture_verified()?;
    if let Some(entry) = repository.observed_entry.as_mut() {
        entry.repository_id = 2;
    }
    must_fail!(
        publication_canonical_bytes(&repository),
        "foreign repository readback"
    );
    let mut step = fixture_verified()?;
    step.save_step_outcome = Some(String::new());
    must_fail!(publication_canonical_bytes(&step), "empty step outcome");
    let mut long = fixture_verified()?;
    long.save_step_outcome = Some("s".repeat(MAX_PUBLICATION_TEXT_BYTES + 1));
    must_fail!(publication_canonical_bytes(&long), "long step outcome");
    Ok(())
}

fn check_rejects_bad_readback() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let key = generation.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = fixture_size()?;
    let entry = fixture_entry(&generation, size);
    let mut malformed = bound_inputs(&key, &digest, size, Some(&bytes), Some(&entry), true);
    malformed.frozen_archive_digest = "not-a-digest";
    must_fail!(decide_readback(&malformed), "malformed frozen digest");
    let mut empty = bound_inputs(&key, &digest, size, Some(&bytes), Some(&entry), true);
    empty.frozen_archive_bytes = 0;
    must_fail!(decide_readback(&empty), "empty frozen candidate");
    let mut bad_key = entry.clone();
    "velnor-gc1-broken".clone_into(&mut bad_key.key);
    let bad_entry = bound_inputs(&key, &digest, size, Some(&bytes), Some(&bad_key), true);
    must_fail!(decide_readback(&bad_entry), "malformed entry key");
    let mut long_ref = entry.clone();
    long_ref.ref_name = "r".repeat(MAX_RECOVERY_TEXT_BYTES + 1);
    let long_entry = bound_inputs(&key, &digest, size, Some(&bytes), Some(&long_ref), true);
    must_fail!(decide_readback(&long_entry), "long entry ref");
    must_fail!(require_empty_restore_target(true), "occupied target");
    must_fail!(
        retain_usable_families(std::slice::from_ref(&key), "velnor-gc1-broken"),
        "malformed failed key"
    );
    must_fail!(
        retain_usable_families(&["velnor-gc1-broken".to_owned()], &key),
        "malformed usable key"
    );
    Ok(())
}

fn check_rejects_bad_encoding() -> Result<(), Box<dyn Error>> {
    let record = fixture_verified()?;
    let bytes = publication_canonical_bytes(&record).map_err(|error| fail(&error.to_string()))?;
    must_fail!(
        parse_publication_bytes(&bytes[..bytes.len() / 2]),
        "truncated record"
    );
    must_fail!(
        parse_publication_bytes(br#"{"schema":1,"schema":1}"#),
        "duplicate key"
    );
    must_fail!(
        parse_publication_bytes(&vec![b' '; MAX_PUBLICATION_JSON_BYTES + 1]),
        "oversized record"
    );
    let mut value = serde_json::to_value(&record).map_err(|_| fail("record not object"))?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("record not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    let padded = serde_json::to_vec(&value).map_err(|_| fail("record not serializable"))?;
    must_fail!(parse_publication_bytes(&padded), "unknown field");
    let empty_measured = MeasuredU64 {
        value: None,
        unknown_reason: None,
    };
    must_fail!(
        empty_measured.validate().map_err(|_| fail("gap")),
        "unknown without reason"
    );
    let double_measured = MeasuredU64 {
        value: Some(1),
        unknown_reason: Some(ObservationGapV1 {
            code: ObservationGapCode::MissingSession,
            detail: "gc1-t26-double".to_owned(),
            affected_recipe_ids: Vec::new(),
            evidence_ref: "actions-cache-inventory".to_owned(),
        }),
    };
    must_fail!(
        double_measured.validate().map_err(|_| fail("gap")),
        "measured with reason"
    );
    if validate_sha256_hex("not-a-digest").is_ok() || validate_sha256_hex(&"A".repeat(64)).is_ok() {
        return Err(fail("malformed digest passed validation"));
    }
    Ok(())
}

#[test]
fn gc1_t26_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_save_requests()?;
    check_rejects_idle_mismatch()?;
    check_rejects_verified_mismatch()?;
    check_rejects_bad_readback()?;
    check_rejects_bad_encoding()?;
    Ok(())
}
