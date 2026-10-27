// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C20: a source-free authority job without entries recovers through the cold
/// path with real nonzero work and never claims a warm exact hit.
pub(crate) fn check_authority_recovers_cold_without_state() -> Result<(), Box<dyn Error>> {
    let generation = paths_generation(
        b"gc1-qualification-paths-authority",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 181, 1, 1811, "authority")?;
    let request = paths_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid(
            "authority job claimed cached state it never verified",
        ));
    }
    if record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("authority job skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    require_nonzero(
        "authority cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("authority native work", warm.measured_native_compilations())?;
    must_fail(
        &require_nonzero("authority warm pass", 0),
        "authority job reported a warm zero-work pass",
    )?;
    Ok(())
}

/// C21: old and new namespace entries are both charged during overlap; the
/// combined inventory admits when it fits and blocks when it overflows, while
/// a complete inventory keeps its exact stored sum.
pub(crate) fn check_overlap_charges_old_and_new() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let old_bytes = 7_200_000_000_u64;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![EntryTupleV1 {
            repository_id: 1,
            cache_id: 2110,
            ref_name: "refs/heads/main".to_owned(),
            key: paths_generation(
                b"gc1-qualification-paths-old",
                b"gc1-qualification-paths-old-group",
            )?
            .cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: old_bytes,
            created_at: "2026-10-25T00:00:00Z".to_owned(),
            last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
        }],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(old_bytes) {
        return Err(invalid("migration inventory lost its old-family bytes"));
    }
    let generation = paths_generation(
        b"gc1-qualification-paths-new",
        b"gc1-qualification-paths-new-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 211, 1, 2111, "writer")?;
    let fit = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        old_bytes,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("fitting migration overlap blocked admission"));
    }
    let overflow = decide_admission(&paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        OPERATING_CEILING_BYTES - bound + 1,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if overflow.decision != AdmissionDecision::Blocked {
        return Err(invalid("overflowing migration overlap admitted"));
    }
    Ok(())
}

/// C21: the old key never exact-hits the new generation, and old and new
/// namespaces keep distinct keys with strict round trips.
pub(crate) fn check_old_key_never_exact_hits() -> Result<(), Box<dyn Error>> {
    let old = paths_generation(
        b"gc1-qualification-paths-old",
        b"gc1-qualification-paths-old-group",
    )?;
    let new = paths_generation(
        b"gc1-qualification-paths-new",
        b"gc1-qualification-paths-new-group",
    )?;
    if old.cache_key() == new.cache_key() {
        return Err(invalid("migration kept a single namespace key"));
    }
    GenerationId::validate_key(&new.cache_key())?;
    if GenerationId::parse_key(&new.cache_key())? != new {
        return Err(invalid("new generation key lost its strict round trip"));
    }
    must_fail(
        &GenerationId::validate_key("bogus-key"),
        "malformed migration key admitted",
    )?;
    let job = paths_job("push", "refs/heads/main", 212, 1, 2121, "reader")?;
    let request = paths_discovery(
        job,
        new.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![paths_entry(
            &old,
            "refs/heads/main",
            2120,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("old namespace entry exact-hit the new generation"));
    }
    let exact = RestoreObservation {
        intended_key: new.cache_key(),
        matched_key: new.cache_key(),
        restored_compatibility: new.compatibility_digest.clone(),
        expected_compatibility: new.compatibility_digest.clone(),
        restored_coverage: new.coverage_digest.clone(),
        expected_coverage: new.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-paths-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-paths-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let mut stale = exact.clone();
    stale.matched_key = old.cache_key();
    must_fail(
        &verify_restore(&stale),
        "old archive imported into trusted new state",
    )?;
    Ok(())
}

/// C21: explicitly owned superseded IDs delete only after the verified new
/// generation restores; unlisted IDs and foreign namespaces stay protected.
pub(crate) fn check_verified_replacement_gates_owned_delete() -> Result<(), Box<dyn Error>> {
    require_qualification_destructive(
        &FixtureCacheRef {
            cache_id: 2130,
            namespace: "velnor-gc1-qualification".to_owned(),
        },
        &[2130],
    )?;
    must_fail(
        &require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 9999,
                namespace: "velnor-gc1-qualification".to_owned(),
            },
            &[2130],
        ),
        "unowned superseded ID admitted for deletion",
    )?;
    must_fail(
        &require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 2130,
                namespace: "production".to_owned(),
            },
            &[2130],
        ),
        "foreign namespace admitted for deletion",
    )?;
    Ok(())
}

/// C22: every active family records its own generation key; sources, tools,
/// and state stay distinct so pressure accounting never merges them.
pub(crate) fn check_all_families_recorded() -> Result<(), Box<dyn Error>> {
    if Gc1Family::ALL.len() != 3 {
        return Err(invalid("cache family inventory drifted"));
    }
    if Gc1Family::Sources.as_str() == Gc1Family::Tools.as_str()
        || Gc1Family::Tools.as_str() == Gc1Family::State.as_str()
        || Gc1Family::Sources.as_str() == Gc1Family::State.as_str()
    {
        return Err(invalid("cache families merged their names"));
    }
    let mut keys = std::collections::BTreeSet::new();
    for family in Gc1Family::ALL {
        let generation = GenerationId {
            family,
            group_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-group"))?,
            compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-compat"))?,
            coverage_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-coverage"))?,
            repair_revision: 0,
        };
        GenerationId::validate_key(&generation.cache_key())?;
        if !keys.insert(generation.cache_key()) {
            return Err(invalid("two families shared one generation key"));
        }
    }
    if keys.len() != 3 {
        return Err(invalid("family pressure lost a contributor"));
    }
    Ok(())
}

/// C22: the repository-wide ceiling counts the full mixed inventory, not just
/// the Rust namespace; an exact fit admits while a one-byte overflow blocks.
#[expect(
    clippy::too_many_lines,
    reason = "mixed-family ceiling keeps inventory, fit, and overflow together"
)]
pub(crate) fn check_repository_ceiling_counts_all_families() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![
            EntryTupleV1 {
                repository_id: 1,
                cache_id: 2210,
                ref_name: "refs/heads/main".to_owned(),
                key: paths_generation(
                    b"gc1-qualification-paths-rust",
                    b"gc1-qualification-paths-group",
                )?
                .cache_key(),
                version: "v1".to_owned(),
                size_in_bytes: 6_000_000_000,
                created_at: "2026-10-25T00:00:00Z".to_owned(),
                last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
            },
            EntryTupleV1 {
                repository_id: 1,
                cache_id: 2211,
                ref_name: "refs/heads/main".to_owned(),
                key: paths_generation(
                    b"gc1-qualification-paths-other",
                    b"gc1-qualification-paths-group",
                )?
                .cache_key(),
                version: "v1".to_owned(),
                size_in_bytes: inventory_sum - 6_000_000_000,
                created_at: "2026-10-25T00:00:00Z".to_owned(),
                last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
            },
        ],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(inventory_sum) {
        return Err(invalid("mixed-family inventory lost its stored sum"));
    }
    let generation = paths_generation(
        b"gc1-qualification-paths-pressure",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 221, 1, 2211, "writer")?;
    let fit = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("exact mixed-family fit blocked admission"));
    }
    if fit.candidate_upper_bound_bytes.value != Some(bound) {
        return Err(invalid("admission lost its qualified upper bound"));
    }
    let over = decide_admission(&paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        inventory_sum + 1,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if over.decision != AdmissionDecision::Blocked {
        return Err(invalid("one-byte mixed-family overflow admitted"));
    }
    Ok(())
}
