// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_degraded_exact_without_inventory() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let mut request = discovery_request(
        generation.clone(),
        vec![exact_entry(7, &generation)],
        Vec::new(),
    )?;
    request.inventory_complete = false;
    let record = decide_discovery(&request)?;
    if record.decision != DiscoveryDecision::ExactHit
        || record.shadow_state != ShadowState::Unknown
        || record.predecessor_candidate.is_some()
    {
        return Err(fail("trusted exact lost its degraded-inventory path"));
    }
    Ok(())
}

#[test]
fn gc1_t24_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_exact_first()?;
    check_single_ranked_predecessor()?;
    check_intent_survives_application_head()?;
    check_degraded_exact_without_inventory()?;
    Ok(())
}

pub(crate) fn check_shadow_refusals() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let forked = discovery_request(
        generation.clone(),
        vec![
            exact_entry(7, &generation),
            entry(
                9,
                &generation.cache_key(),
                "refs/pull/9/merge",
                "2026-10-25T01:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        Vec::new(),
    )?;
    let shadowed = decide_discovery(&forked)?;
    if shadowed.decision != DiscoveryDecision::RefusedShadow
        || shadowed.shadow_state != ShadowState::ConflictingVisibleRef
        || shadowed.exact_candidate.is_some()
    {
        return Err(fail("cross-ref exact shadow missed its refusal"));
    }
    let longer_key = format!("{}-extra-suffix", primary_prefix(&generation));
    let current_shadow = discovery_request(
        generation.clone(),
        vec![
            entry(
                11,
                &generation.cache_key(),
                "refs/pull/9/merge",
                "2026-10-25T00:00:00Z",
                EntryProvenance::Trusted,
            ),
            entry(
                12,
                &longer_key,
                "refs/heads/main",
                "2026-10-25T01:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        Vec::new(),
    )?;
    let masked = decide_discovery(&current_shadow)?;
    if masked.decision != DiscoveryDecision::RefusedShadow {
        return Err(fail("current-ref longer-key shadow missed its refusal"));
    }
    check_fallback_shadow_report()?;
    Ok(())
}

fn check_fallback_shadow_report() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let spread = discovery_request(
        generation,
        vec![
            entry(
                13,
                &format!("{}-aaa", primary_prefix(&fixture_generation()?)),
                "refs/heads/main",
                "2026-10-25T00:00:00Z",
                EntryProvenance::Trusted,
            ),
            entry(
                14,
                &format!("{}-bbb", primary_prefix(&fixture_generation()?)),
                "refs/pull/9/merge",
                "2026-10-25T01:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        Vec::new(),
    )?;
    let exact_refs = std::collections::BTreeSet::new();
    let longer_refs = std::collections::BTreeSet::from([
        "refs/heads/main".to_owned(),
        "refs/pull/9/merge".to_owned(),
    ]);
    if classify_shadow(&exact_refs, &longer_refs, "refs/heads/main")
        != ShadowState::ConflictingVisibleRef
    {
        return Err(fail("spread fallback candidates missed their conflict"));
    }
    let cold = decide_discovery(&spread)?;
    if cold.decision != DiscoveryDecision::ColdMissing
        || cold.shadow_state != ShadowState::ConflictingVisibleRef
    {
        return Err(fail("cold path lost its shadow report"));
    }
    Ok(())
}

pub(crate) fn check_provenance_refusals() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    for provenance in [
        EntryProvenance::Ambiguous,
        EntryProvenance::Untrusted,
        EntryProvenance::Unknown,
    ] {
        let request = discovery_request(
            generation.clone(),
            vec![entry(
                7,
                &generation.cache_key(),
                "refs/heads/main",
                "2026-10-25T00:00:00Z",
                provenance,
            )],
            Vec::new(),
        )?;
        let refused = decide_discovery(&request)?;
        if refused.decision != DiscoveryDecision::RefusedProvenance
            || refused.exact_candidate.is_some()
        {
            return Err(fail("distrusted exact missed its refusal"));
        }
    }
    let longer_key = format!("{}-extra-suffix", primary_prefix(&generation));
    let bystander = discovery_request(
        generation.clone(),
        vec![
            exact_entry(7, &generation),
            entry(
                8,
                &longer_key,
                "refs/heads/main",
                "2026-10-25T01:00:00Z",
                EntryProvenance::Ambiguous,
            ),
        ],
        Vec::new(),
    )?;
    let refused = decide_discovery(&bystander)?;
    if refused.decision != DiscoveryDecision::RefusedProvenance {
        return Err(fail("ambiguous fallback candidate missed its refusal"));
    }
    let mut degraded = discovery_request(
        generation.clone(),
        vec![exact_entry(7, &generation)],
        Vec::new(),
    )?;
    degraded.inventory_complete = false;
    degraded.entries[0].provenance = EntryProvenance::Untrusted;
    let unavailable = decide_discovery(&degraded)?;
    if unavailable.decision != DiscoveryDecision::ColdUnavailable {
        return Err(fail("distrusted degraded exact missed unavailable"));
    }
    Ok(())
}

pub(crate) fn check_restore_and_coverage() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let manifest = digest_b3_typed(b"gc1-t24-manifest");
    let good = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: manifest.clone(),
        intended_manifest: manifest,
    };
    verify_restore(&good)?;
    let mut key_drift = good.clone();
    key_drift.matched_key = format!("{}-r1", primary_prefix(&generation));
    must_fail!(verify_restore(&key_drift), "post-restore key drift");
    let mut coverage_drift = good.clone();
    coverage_drift.restored_coverage = digest_b3_typed(b"gc1-t24-other-coverage");
    must_fail!(verify_restore(&coverage_drift), "completed coverage drift");
    let mut compat_drift = good.clone();
    compat_drift.restored_compatibility = digest_b3_typed(b"gc1-t24-other-compat");
    must_fail!(verify_restore(&compat_drift), "compatibility drift");
    let mut manifest_drift = good.clone();
    manifest_drift.restored_manifest = digest_b3_typed(b"gc1-t24-other-manifest");
    must_fail!(verify_restore(&manifest_drift), "manifest drift");
    let mut empty = good.clone();
    empty.matched_key = String::new();
    must_fail!(verify_restore(&empty), "empty matched key");
    Ok(())
}
