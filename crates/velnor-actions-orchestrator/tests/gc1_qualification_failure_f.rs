// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C16: two occupied generation slots exhaust the repair budget until the
/// exact corrupt entry is deleted; planned deletion is never credited and
/// active coverage is never deleted to manufacture space.
#[expect(
    clippy::too_many_lines,
    reason = "slot repair covers exhaustion, credit, and admission"
)]
pub(crate) fn check_admission_slots_and_repair() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation_a = failure_generation(b"gc1-qualification-failure-slot-a")?;
    let generation_b = failure_generation(b"gc1-qualification-failure-slot-b")?;
    let job = failure_job("push", "refs/heads/main", 162, 1, 1621, "writer")?;
    let full = failure_admission(
        job.clone(),
        generation_b.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        MAX_GENERATION_SLOTS,
    );
    let blocked_slots = decide_admission(&full).map_err(|error| invalid(&error.to_string()))?;
    if blocked_slots.decision != AdmissionDecision::Blocked {
        return Err(invalid("exhausted slots admitted a third generation"));
    }
    if !blocked_slots
        .blocked_reasons
        .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(invalid("exhausted slots missed their reason"));
    }
    let mut credited = failure_admission(
        job.clone(),
        generation_b.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    credited.claimed_freed_bytes = 1;
    must_fail(
        &decide_admission(&credited).map(|_| ()),
        "planned deletion credited before confirmation",
    )?;
    require_qualification_destructive(
        &crate::gc1_qualification_support::FixtureCacheRef {
            cache_id: 1621,
            namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
        },
        &[1621],
    )?;
    must_fail(
        &require_qualification_destructive(
            &crate::gc1_qualification_support::FixtureCacheRef {
                cache_id: 1620,
                namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
            },
            &[1621],
        ),
        "active coverage deleted to manufacture space",
    )?;
    let retained = retain_usable_families(
        &[generation_a.cache_key(), generation_b.cache_key()],
        &generation_b.cache_key(),
    )?;
    if retained != vec![generation_a.cache_key()] {
        return Err(invalid("slot repair dropped the usable generation"));
    }
    let freed = failure_admission(
        job,
        generation_b,
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    let admitted = decide_admission(&freed).map_err(|error| invalid(&error.to_string()))?;
    if admitted.decision != AdmissionDecision::Admitted {
        return Err(invalid("confirmed repair blocked slot admission"));
    }
    Ok(())
}

/// C16: the verified publication keeps the final stored size inside the
/// preflight envelope, and a pending write without readback never counts as
/// publication.
pub(crate) fn check_verified_bytes_stay_inside_envelope() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let job = failure_job("push", "refs/heads/main", 163, 1, 1631, "writer")?;
    let verified = PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: job.clone(),
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-qualification-failure-proof"),
        admission_digest: digest_b3_typed(b"gc1-qualification-failure-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(ObservedEntry {
            repository_id: 1,
            cache_id: 1631,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: size,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
        }),
        readback_outcome: ReadbackOutcome::Verified,
        readback_archive_digest: Some(digest),
        readback_manifest_digest: Some(digest_b3_typed(b"gc1-qualification-failure-manifest")),
        actual_stored_bytes: measured_bytes(size),
        active_intent_digest: digest_b3_typed(b"gc1-qualification-failure-intent"),
    };
    if classify_publication(&verified)? != PublicationOutcome::Verified {
        return Err(invalid("verified publication lost its verdict"));
    }
    let mut drifted = verified.clone();
    drifted.actual_stored_bytes = measured_bytes(size + 1);
    must_fail(
        &classify_publication(&drifted).map(|_| ()),
        "stored size escaped its preflight envelope",
    )?;
    let mut unknown = verified.clone();
    unknown.actual_stored_bytes = unknown_bytes("gc1-qualification-failure-stored")
        .map_err(|error| invalid(&error.to_string()))?;
    must_fail(
        &classify_publication(&unknown).map(|_| ()),
        "uncertain stored size admitted as verified",
    )?;
    let pending = PublicationV1 {
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: verified.observed_entry.clone(),
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: measured_bytes(size),
        ..failure_denied_record()?
    };
    must_fail(
        &classify_publication(&pending).map(|_| ()),
        "pending write counted as publication",
    )?;
    Ok(())
}

pub(crate) fn failure_intent(head_sha: &str, coverage_ids: Vec<String>) -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: head_sha.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-qualification-failure-config"),
        active_group_ids: vec![digest_b3(b"gc1-qualification-failure-group")],
        active_coverage_ids: coverage_ids,
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-qualification-failure-intent-evidence"),
    }
}

/// C17: coverage growth under unchanged compatibility keeps both old and new
/// required coverage valid while the compatible prior state restores exactly.
pub(crate) fn check_coverage_growth_keeps_prior() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    let grown = coverage_variant("0.1.0", "0.2.0", Some("tokio"), LINUX_TARGET, None, 'f')?;
    require_valid_digests(&base)?;
    require_valid_digests(&grown)?;
    require_same_coverage(&base, &base.clone())?;
    require_changed_coverage(&base, &grown)?;
    let old_generation = failure_generation(b"gc1-qualification-failure-coverage-old")?;
    let new_generation = failure_generation(b"gc1-qualification-failure-coverage-new")?;
    if old_generation.compatibility_digest != new_generation.compatibility_digest {
        return Err(invalid("coverage growth moved compatibility"));
    }
    if old_generation.cache_key() == new_generation.cache_key() {
        return Err(invalid("coverage growth kept the old key"));
    }
    let job = failure_job("push", "refs/heads/main", 171, 1, 1711, "reader")?;
    let request = failure_discovery(
        job,
        old_generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &old_generation,
            "refs/heads/main",
            1710,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("compatible prior state lost its restore"));
    }
    let observation = RestoreObservation {
        intended_key: old_generation.cache_key(),
        matched_key: old_generation.cache_key(),
        restored_compatibility: old_generation.compatibility_digest.clone(),
        expected_compatibility: new_generation.compatibility_digest.clone(),
        restored_coverage: old_generation.coverage_digest.clone(),
        expected_coverage: old_generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
    };
    verify_restore(&observation).map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}
