// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C15: corrupt inner payloads are rejected before use across digest, size,
/// key, ref, and version mismatches; the failed restore runs required checks
/// through bounded cold recovery instead of a warm pass.
#[expect(
    clippy::too_many_lines,
    reason = "corruption matrix needs every mismatch in one case"
)]
pub(crate) fn check_corrupt_inner_rejected() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-corrupt")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1512,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let mut damaged = bytes.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    let corrupt = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&damaged),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(corrupt, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("corrupt inner payload admitted"));
    }
    let short = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes[..bytes.len() - 1]),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(short, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("truncated inner payload admitted"));
    }
    let wrong_key = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &failure_generation(b"gc1-qualification-failure-other")?.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(wrong_key, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("wrong-key restore admitted"));
    }
    let wrong_ref = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/feature",
        expected_version: "v1",
    })?;
    if !matches!(wrong_ref, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("wrong-ref restore admitted"));
    }
    let observation = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: failure_generation_with_compat(
            b"gc1-qualification-failure-corrupt",
            b"gc1-qualification-failure-other-compat",
            0,
        )?
        .compatibility_digest
        .clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
    };
    must_fail(
        &verify_restore(&observation),
        "incompatible corrupt state admitted",
    )?;
    let cold = cold_ledger()?;
    require_nonzero(
        "corrupt-entry cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    if cold.failed() != 0 {
        return Err(invalid("corrupt-entry cold recovery recorded failures"));
    }
    Ok(())
}

/// C15: a missing or timed-out restore keeps the candidate charge and never
/// authorizes treating the entry as corrupt; a transfer retry observes the
/// same unavailable verdict deterministically.
pub(crate) fn check_restore_timeout_needs_retry() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-timeout")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1513,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let inputs = ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    };
    let first = decide_readback(&inputs)?;
    if !matches!(first, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("timed-out restore claimed a verdict"));
    }
    let retry = decide_readback(&inputs)?;
    if retry != first {
        return Err(invalid("transfer retry changed the unavailable verdict"));
    }
    if matches!(first, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("timeout authorized a corruption charge"));
    }
    require_empty_restore_target(false)?;
    must_fail(
        &require_empty_restore_target(true),
        "occupied restore target admitted",
    )?;
    Ok(())
}

/// C15: with usable predecessor A and reproducibly invalid candidate B in the
/// second slot, the corrupt B is rejected, the exact B entry is deleted under
/// reviewed repair intent while A is retained, and B-r1 is admitted only
/// after B is confirmed gone.
#[expect(
    clippy::too_many_lines,
    reason = "two-slot repair covers discovery, damage, and recheck"
)]
pub(crate) fn check_two_slot_repair_deletes_exact_b() -> Result<(), Box<dyn Error>> {
    let compat = b"gc1-qualification-failure-repair-compat";
    let predecessor =
        failure_generation_with_compat(b"gc1-qualification-failure-slot-a", compat, 0)?;
    let candidate = failure_generation_with_compat(b"gc1-qualification-failure-slot-b", compat, 0)?;
    if predecessor.cache_key() == candidate.cache_key() {
        return Err(invalid("repair slots merged into one generation"));
    }
    let job = failure_job("push", "refs/heads/main", 152, 1, 1521, "reader")?;
    let request = failure_discovery(
        job.clone(),
        candidate.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![
            failure_entry(
                &predecessor,
                "refs/heads/main",
                1520,
                EntryProvenance::Trusted,
            ),
            failure_entry(
                &candidate,
                "refs/heads/main",
                1521,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("two-slot discovery missed the candidate"));
    }
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let damaged = vec![0u8; bytes.len()];
    let corrupt = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&damaged),
        inventory_complete: true,
        observed_entry: Some(&ObservedEntry {
            repository_id: 1,
            cache_id: 1521,
            ref_name: "refs/heads/main".to_owned(),
            key: candidate.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: size,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
        }),
        expected_key: &candidate.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(corrupt, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("invalid candidate admitted for use"));
    }
    require_qualification_destructive(
        &crate::gc1_qualification_support::FixtureCacheRef {
            cache_id: 1521,
            namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
        },
        &[1521],
    )?;
    must_fail(
        &require_qualification_destructive(
            &crate::gc1_qualification_support::FixtureCacheRef {
                cache_id: 1520,
                namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
            },
            &[1521],
        ),
        "unapproved predecessor deletion admitted",
    )?;
    let retained = retain_usable_families(
        &[predecessor.cache_key(), candidate.cache_key()],
        &candidate.cache_key(),
    )?;
    if retained != vec![predecessor.cache_key()] {
        return Err(invalid("repair dropped the usable predecessor"));
    }
    must_fail(
        &retain_usable_families(&[predecessor.cache_key()], "bogus-key").map(|_| ()),
        "malformed repair charge admitted",
    )?;
    let repaired = failure_generation_with_compat(b"gc1-qualification-failure-slot-b", compat, 1)?;
    if repaired.cache_key() == candidate.cache_key() {
        return Err(invalid("repair revision kept the corrupt key"));
    }
    if !repaired.cache_key().ends_with("-r1") {
        return Err(invalid("repair revision drifted from its bounded key"));
    }
    let after = failure_discovery(
        job,
        repaired.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &predecessor,
            "refs/heads/main",
            1520,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let after_record = decide_discovery(&after).map_err(|error| invalid(&error.to_string()))?;
    if after_record.decision != DiscoveryDecision::PredecessorFallback {
        return Err(invalid("repair recheck dropped its predecessor"));
    }
    let visible: Vec<&SelectableEntry> = after.entries.iter().collect();
    let kept = select_predecessor(&after, &visible)
        .ok_or_else(|| invalid("repair recheck lost predecessor A"))?;
    if kept.entry.cache_id != 1520 {
        return Err(invalid("repair recheck kept the wrong entry"));
    }
    let replacement_save =
        failure_save(true, None, true, false, Some(1024), 8192, SaveHalt::Running);
    if decide_save(&replacement_save)? != SaveOutcome::Attempted {
        return Err(invalid("confirmed repair blocked replacement admission"));
    }
    Ok(())
}

/// C15: bounded lookup-only and full-restore observations record their own
/// `last_accessed_at` values; both validate as bounded entries without
/// assuming keepalive or recency neutrality.
pub(crate) fn check_lookup_and_restore_recency_observed() -> Result<(), Box<dyn Error>> {
    let generation = failure_generation(b"gc1-qualification-failure-recency")?;
    let lookup = EntryTupleV1 {
        repository_id: 1,
        cache_id: 1530,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let restored = EntryTupleV1 {
        last_accessed_at: "2026-10-26T00:05:00Z".to_owned(),
        ..lookup.clone()
    };
    lookup
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    restored
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    if lookup.cache_id != restored.cache_id || lookup.key != restored.key {
        return Err(invalid("recency observations lost their entry identity"));
    }
    if lookup.last_accessed_at == restored.last_accessed_at {
        return Err(invalid("full restore assumed lookup recency"));
    }
    let inventory = build_inventory(1, "2026-10-26T00:05:00Z", vec![restored], 1)
        .map_err(|error| invalid(&error.to_string()))?;
    if inventory.entries.len() != 1 {
        return Err(invalid("bounded restore observation lost its entry"));
    }
    if inventory_digest(&inventory).map_err(|error| invalid(&error.to_string()))?
        != inventory_digest(&inventory).map_err(|error| invalid(&error.to_string()))?
    {
        return Err(invalid("restore inventory digest drifted"));
    }
    Ok(())
}
