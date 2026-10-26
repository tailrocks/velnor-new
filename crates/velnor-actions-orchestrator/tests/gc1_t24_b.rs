// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_boundaries_hold() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let mut foreign = exact_entry(7, &generation);
    foreign.repository_id = 2;
    let cross_repo = discovery_request(generation.clone(), vec![foreign], Vec::new())?;
    let cold = decide_discovery(&cross_repo)?;
    if cold.decision != DiscoveryDecision::ColdMissing {
        return Err(fail("foreign repository exact crossed its boundary"));
    }
    let mut wrong_version = exact_entry(7, &generation);
    "v9".clone_into(&mut wrong_version.version);
    let versioned = discovery_request(generation.clone(), vec![wrong_version], Vec::new())?;
    if decide_discovery(&versioned)?.decision != DiscoveryDecision::ColdMissing {
        return Err(fail("version drift crossed its boundary"));
    }
    let mut wrong_ref = exact_entry(7, &generation);
    "refs/heads/feature".clone_into(&mut wrong_ref.ref_name);
    let scoped = discovery_request(generation.clone(), vec![wrong_ref], Vec::new())?;
    if decide_discovery(&scoped)?.decision != DiscoveryDecision::ColdMissing {
        return Err(fail("out-of-scope ref crossed its boundary"));
    }
    let incompatible = predecessor_generation(b"gc1-t24-coverage")?;
    let mut drifted = incompatible.clone();
    drifted.compatibility_digest = digest_b3_typed(b"gc1-t24-other-compat");
    let compat_request = discovery_request(
        generation,
        vec![entry(
            51,
            &drifted.cache_key(),
            "refs/heads/main",
            "2026-10-25T00:00:00Z",
            EntryProvenance::Trusted,
        )],
        Vec::new(),
    )?;
    let compat_visible: Vec<&SelectableEntry> = compat_request.entries.iter().collect();
    if select_predecessor(&compat_request, &compat_visible).is_some() {
        return Err(fail("incompatible generation crossed its boundary"));
    }
    let malformed = discovery_request(
        fixture_generation()?,
        vec![entry(
            52,
            "not-a-cache-key",
            "refs/heads/main",
            "2026-10-25T00:00:00Z",
            EntryProvenance::Trusted,
        )],
        Vec::new(),
    )?;
    if decide_discovery(&malformed)?.decision != DiscoveryDecision::ColdMissing {
        return Err(fail("malformed key escaped its filter"));
    }
    Ok(())
}

fn check_intent_blocks_mutation() -> Result<(), Box<dyn Error>> {
    let previous = fixture_intent(&format!("{:040x}", 0x1111))?;
    let unknown = recheck_intent(&previous, None)?;
    if unknown != IntentRecheck::MutationBlocked || intent_authorizes_mutation(unknown) {
        return Err(fail("unknown intent missed its mutation block"));
    }
    let mut changed_config = fixture_observation(&format!("{:040x}", 0x3333));
    changed_config.configuration_digest = digest_b3_typed(b"gc1-t24-other-config");
    if recheck_intent(&previous, Some(&changed_config))? != IntentRecheck::CoverageChanged {
        return Err(fail("configuration drift missed its change"));
    }
    let mut changed_groups = fixture_observation(&format!("{:040x}", 0x3333));
    changed_groups.active_group_ids = vec!["group-a".to_owned()];
    if recheck_intent(&previous, Some(&changed_groups))? != IntentRecheck::CoverageChanged {
        return Err(fail("group drift missed its change"));
    }
    let mut changed_coverage = fixture_observation(&format!("{:040x}", 0x3333));
    changed_coverage.active_coverage_ids = vec!["coverage-b".to_owned()];
    let changed = recheck_intent(&previous, Some(&changed_coverage))?;
    if changed != IntentRecheck::CoverageChanged || intent_authorizes_mutation(changed) {
        return Err(fail("coverage drift missed its mutation block"));
    }
    let mut zero_repo = intent_draft(&format!("{:040x}", 0x1111));
    zero_repo.repository_id = 0;
    must_fail!(derive_intent(zero_repo), "zero repository intent",);
    let mut bad_sha = intent_draft(&format!("{:040x}", 0x1111));
    "not-a-sha".clone_into(&mut bad_sha.observed_head_sha);
    must_fail!(derive_intent(bad_sha), "malformed head sha",);
    let mut unsorted = fixture_intent(&format!("{:040x}", 0x1111))?;
    unsorted.active_group_ids = vec!["group-b".to_owned(), "group-a".to_owned()];
    must_fail!(intent_canonical_bytes(&unsorted), "unsorted intent groups");
    Ok(())
}

fn check_rejects_invalid_records() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let request = discovery_request(
        generation.clone(),
        vec![exact_entry(7, &generation)],
        Vec::new(),
    )?;
    let record = decide_discovery(&request)?;
    let mut bad_schema = record.clone();
    bad_schema.schema = 2;
    must_fail!(discovery_canonical_bytes(&bad_schema), "discovery schema");
    let mut unsorted_refs = record.clone();
    unsorted_refs.allowed_refs = vec!["refs/pull/9/merge".to_owned(), "refs/heads/main".to_owned()];
    must_fail!(
        discovery_canonical_bytes(&unsorted_refs),
        "unsorted allowed refs"
    );
    let mut empty_reason = record.clone();
    empty_reason.reason = String::new();
    must_fail!(discovery_canonical_bytes(&empty_reason), "empty reason");
    let mut missing_exact = record.clone();
    missing_exact.exact_candidate = None;
    must_fail!(
        discovery_canonical_bytes(&missing_exact),
        "exact decision without candidate",
    );
    let mut cold_with_candidate = record.clone();
    cold_with_candidate.decision = DiscoveryDecision::ColdMissing;
    must_fail!(
        discovery_canonical_bytes(&cold_with_candidate),
        "cold decision with candidate",
    );
    let mut two_candidates = record.clone();
    two_candidates.predecessor_candidate = two_candidates.exact_candidate.clone();
    must_fail!(
        discovery_canonical_bytes(&two_candidates),
        "two retained candidates",
    );
    let mut degraded_predecessor = record.clone();
    degraded_predecessor.inventory_complete = false;
    degraded_predecessor.shadow_state = ShadowState::Unknown;
    degraded_predecessor.predecessor_candidate = degraded_predecessor.exact_candidate.clone();
    degraded_predecessor.exact_candidate = None;
    degraded_predecessor.decision = DiscoveryDecision::PredecessorFallback;
    must_fail!(
        discovery_canonical_bytes(&degraded_predecessor),
        "predecessor without inventory",
    );
    let bytes = discovery_canonical_bytes(&record)?;
    must_fail!(
        parse_discovery_bytes(&bytes[..bytes.len() / 2]),
        "truncated discovery",
    );
    must_fail!(
        parse_discovery_bytes(br#"{"schema":1,"schema":1}"#),
        "duplicate key",
    );
    let mut value = serde_json::to_value(&record)?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("discovery not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_discovery_bytes(&serde_json::to_vec(&value)?),
        "unknown field",
    );
    check_rejects_invalid_intent_records()?;
    Ok(())
}

fn check_rejects_invalid_intent_records() -> Result<(), Box<dyn Error>> {
    let intent = fixture_intent(&format!("{:040x}", 0x1111))?;
    let intent_bytes = intent_canonical_bytes(&intent)?;
    must_fail!(
        parse_intent_bytes(&intent_bytes[..intent_bytes.len() / 2]),
        "truncated intent",
    );
    let mut intent_value = serde_json::to_value(&intent)?;
    intent_value
        .as_object_mut()
        .ok_or_else(|| fail("intent not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_intent_bytes(&serde_json::to_vec(&intent_value)?),
        "unknown intent field",
    );
    Ok(())
}

#[test]
fn gc1_t24_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_shadow_refusals()?;
    check_provenance_refusals()?;
    check_restore_and_coverage()?;
    check_boundaries_hold()?;
    check_intent_blocks_mutation()?;
    check_rejects_invalid_records()?;
    Ok(())
}
