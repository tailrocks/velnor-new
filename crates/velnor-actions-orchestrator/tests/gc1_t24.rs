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
    reason = "T24 modules are private to cache_keys; the test exercises them by path include"
)]
#[path = "../src/cache_discovery.rs"]
mod cache_discovery;

#[expect(
    unreachable_pub,
    reason = "T24 modules are private to cache_keys; the test exercises them by path include"
)]
#[path = "../src/cache_intent.rs"]
mod cache_intent;

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    ShadowState, classify_shadow, decide_discovery, discovery_canonical_bytes, discovery_digest,
    parse_discovery_bytes, primary_prefix, select_exact, select_predecessor, verify_restore,
};
use cache_intent::{
    IntentObservation, IntentRecheck, derive_intent, intent_authorizes_mutation,
    intent_canonical_bytes, intent_digest, parse_intent_bytes, recheck_intent,
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
        plan_digest: Digest::parse(&digest_b3(b"gc1-t24-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t24-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t24-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t24-coverage"))?,
        repair_revision: 0,
    })
}

fn predecessor_generation(seed: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    let base = fixture_generation()?;
    Ok(GenerationId {
        coverage_digest: Digest::parse(&digest_b3(seed))?,
        ..base
    })
}

fn entry(
    cache_id: u64,
    key: &str,
    ref_name: &str,
    created_at: &str,
    provenance: EntryProvenance,
) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: ref_name.to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: created_at.to_owned(),
        provenance,
    }
}

fn exact_entry(cache_id: u64, generation: &GenerationId) -> SelectableEntry {
    entry(
        cache_id,
        &generation.cache_key(),
        "refs/heads/main",
        "2026-10-25T00:00:00Z",
        EntryProvenance::Trusted,
    )
}

fn discovery_request(
    generation: GenerationId,
    entries: Vec<SelectableEntry>,
    active_coverage: Vec<String>,
) -> Result<DiscoveryRequest, Box<dyn Error>> {
    Ok(DiscoveryRequest {
        job: fixture_job()?,
        generation,
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned(), "refs/pull/9/merge".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-t24-inventory"),
        inventory_complete: true,
        entries,
        active_coverage_ids: active_coverage,
    })
}

fn intent_draft(head: &str) -> cache_intent::IntentDraft {
    cache_intent::IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: head.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-t24-config"),
        active_group_ids: vec!["group-a".to_owned(), "group-b".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
        derived_at: "2026-10-25T02:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-t24-evidence"),
    }
}

fn fixture_intent(head: &str) -> Result<cache_intent::ActiveIntentV1, Box<dyn Error>> {
    Ok(derive_intent(intent_draft(head))?)
}

fn fixture_observation(head: &str) -> IntentObservation {
    IntentObservation {
        observed_head_sha: head.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-t24-config"),
        active_group_ids: vec!["group-a".to_owned(), "group-b".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

fn check_exact_first() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let predecessor = predecessor_generation(b"gc1-t24-older-coverage")?;
    let request = discovery_request(
        generation.clone(),
        vec![
            exact_entry(7, &generation),
            entry(
                3,
                &predecessor.cache_key(),
                "refs/heads/main",
                "2026-10-24T00:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        vec![predecessor.coverage_digest.as_str().to_owned()],
    )?;
    let record = decide_discovery(&request)?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(fail("exact requested key lost first position"));
    }
    let candidate = record
        .exact_candidate
        .as_ref()
        .ok_or_else(|| fail("exact hit lost its candidate"))?;
    if candidate.entry.key != generation.cache_key()
        || candidate.generation != generation
        || record.predecessor_candidate.is_some()
    {
        return Err(fail("exact hit kept a predecessor candidate"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(fail("clean inventory reported a shadow"));
    }
    let bytes = discovery_canonical_bytes(&record)?;
    if parse_discovery_bytes(&bytes)? != record {
        return Err(fail("discovery strict round trip drifted"));
    }
    if discovery_digest(&record)? != discovery_digest(&parse_discovery_bytes(&bytes)?)? {
        return Err(fail("discovery digest drifted across round trip"));
    }
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let direct =
        select_exact(&request, &visible).ok_or_else(|| fail("direct exact selection missed"))?;
    if direct.entry.cache_id != 7 {
        return Err(fail("direct exact selection missed the authorized key"));
    }
    if primary_prefix(&generation).is_empty()
        || generation.cache_key() != format!("{}-r0", primary_prefix(&generation))
    {
        return Err(fail("primary prefix drifted from the exact key"));
    }
    Ok(())
}

fn check_single_ranked_predecessor() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let active = predecessor_generation(b"gc1-t24-active-coverage")?;
    let stale = predecessor_generation(b"gc1-t24-stale-coverage")?;
    let request = discovery_request(
        generation,
        vec![
            entry(
                31,
                &stale.cache_key(),
                "refs/heads/main",
                "2026-10-20T00:00:00Z",
                EntryProvenance::Trusted,
            ),
            entry(
                33,
                &active.cache_key(),
                "refs/heads/main",
                "2026-10-24T00:00:00Z",
                EntryProvenance::Trusted,
            ),
            entry(
                32,
                &stale.cache_key(),
                "refs/pull/9/merge",
                "2026-10-10T00:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        vec![active.coverage_digest.as_str().to_owned()],
    )?;
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let predecessor = select_predecessor(&request, &visible)
        .ok_or_else(|| fail("compatible predecessor missed selection"))?;
    if predecessor.entry.cache_id != 33 {
        return Err(fail("active coverage intent lost predecessor ranking"));
    }
    let record = decide_discovery(&request)?;
    if record.decision != DiscoveryDecision::PredecessorFallback || record.exact_candidate.is_some()
    {
        return Err(fail("predecessor fallback lost its decision"));
    }
    if record
        .predecessor_candidate
        .as_ref()
        .ok_or_else(|| fail("fallback lost its candidate"))?
        .entry
        .cache_id
        != 33
    {
        return Err(fail("fallback kept more than the ranked predecessor"));
    }
    let tie_request = discovery_request(
        fixture_generation()?,
        vec![
            entry(
                41,
                &stale.cache_key(),
                "refs/heads/main",
                "2026-10-22T00:00:00Z",
                EntryProvenance::Trusted,
            ),
            entry(
                42,
                &stale.cache_key(),
                "refs/heads/main",
                "2026-10-21T00:00:00Z",
                EntryProvenance::Trusted,
            ),
        ],
        Vec::new(),
    )?;
    let tie_visible: Vec<&SelectableEntry> = tie_request.entries.iter().collect();
    let tie = select_predecessor(&tie_request, &tie_visible)
        .ok_or_else(|| fail("tied predecessor missed selection"))?;
    if tie.entry.cache_id != 42 {
        return Err(fail("creation-time tie-break drifted"));
    }
    Ok(())
}

fn check_intent_survives_application_head() -> Result<(), Box<dyn Error>> {
    let previous = fixture_intent(&format!("{:040x}", 0x1111))?;
    let current = fixture_observation(&format!("{:040x}", 0x2222));
    let recheck = recheck_intent(&previous, Some(&current))?;
    if recheck != IntentRecheck::CoverageUnchanged {
        return Err(fail("application-only HEAD change retired valid coverage"));
    }
    if !intent_authorizes_mutation(recheck) {
        return Err(fail("unchanged coverage blocked its mutation"));
    }
    let bytes = intent_canonical_bytes(&previous)?;
    if parse_intent_bytes(&bytes)? != previous {
        return Err(fail("intent strict round trip drifted"));
    }
    if intent_digest(&previous)? != intent_digest(&parse_intent_bytes(&bytes)?)? {
        return Err(fail("intent digest drifted across round trip"));
    }
    let mut reordered_draft = intent_draft(&format!("{:040x}", 0x1111));
    reordered_draft.active_group_ids = vec!["group-b".to_owned(), "group-a".to_owned()];
    let reordered = derive_intent(reordered_draft)?;
    if intent_digest(&reordered)? != intent_digest(&previous)? {
        return Err(fail("intent digest drifted across group order"));
    }
    Ok(())
}

#[path = "gc1_t24_a.rs"]
mod gc1_t24_a;
#[path = "gc1_t24_b.rs"]
mod gc1_t24_b;
pub(crate) use gc1_t24_a::*;
