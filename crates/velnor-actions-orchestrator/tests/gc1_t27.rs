// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    unreachable_pub,
    reason = "T27 cleanup policy module is private to the crate; the test exercises it by path include"
)]
#[path = "../src/cache_cleanup_policy.rs"]
mod cache_cleanup_policy;

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use self::cache_cleanup_policy::{
    CLEANUP_SCHEMA, CandidateEntry, CleanupMode, CleanupRequest, DeleteOutcome,
    MAX_CLEANUP_CANDIDATES, MAX_CLEANUP_TEXT_BYTES, ReadbackState, RecheckOutcome,
    admit_repair_generation, confirm_absent, decide_cleanup, release_charges,
    require_reproducibly_corrupt, validate_candidate_ids,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_generation(tag: &str, repair_revision: u64) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t27-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t27-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(format!("gc1-t27-{tag}").as_bytes()))?,
        repair_revision,
    })
}

fn fixture_entry(cache_id: u64, generation: &GenerationId) -> CandidateEntry {
    CandidateEntry {
        repository_id: 1,
        cache_id,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 1024 * cache_id,
    }
}

fn superseded_request() -> Result<(CleanupRequest, String, String), Box<dyn Error>> {
    let old_a = fixture_generation("coverage-a", 0)?;
    let old_b = fixture_generation("coverage-b", 0)?;
    let replacement = fixture_generation("coverage-new", 0)?;
    let request = CleanupRequest {
        repository_id: 1,
        ownership_repository_id: 1,
        mode: CleanupMode::SupersededRetirement,
        candidates: vec![fixture_entry(11, &old_a), fixture_entry(12, &old_b)],
        candidate_ids: vec![11, 12],
        intent_current: true,
        intent_repository_id: 1,
        replacement_key: Some(replacement.cache_key()),
        active_keys: Vec::new(),
        repair_authorized: false,
        repair_bad_cache_id: None,
        retained_good_key: None,
        bad_readback: None,
    };
    Ok((request, old_a.cache_key(), old_b.cache_key()))
}

fn repair_request() -> Result<(CleanupRequest, CandidateEntry), Box<dyn Error>> {
    let good = fixture_generation("coverage-a", 0)?;
    let bad = fixture_generation("coverage-b", 0)?;
    let bad_entry = fixture_entry(12, &bad);
    let request = CleanupRequest {
        repository_id: 1,
        ownership_repository_id: 1,
        mode: CleanupMode::CorruptRepair,
        candidates: vec![fixture_entry(11, &good), bad_entry.clone()],
        candidate_ids: vec![11, 12],
        intent_current: true,
        intent_repository_id: 1,
        replacement_key: None,
        active_keys: Vec::new(),
        repair_authorized: true,
        repair_bad_cache_id: Some(12),
        retained_good_key: Some(good.cache_key()),
        bad_readback: Some(ReadbackState::Corrupt {
            evidence: "restored-digest-mismatch".to_owned(),
        }),
    };
    Ok((request, bad_entry))
}

fn check_superseded_retires_only_superseded() -> Result<(), Box<dyn Error>> {
    let (request, _, _) = superseded_request()?;
    let plan = decide_cleanup(&request)?;
    if plan.deletable_ids != vec![11, 12] {
        return Err(fail("superseded retirement dropped an owned entry"));
    }
    if plan.entry_results.len() != 2 {
        return Err(fail("superseded retirement lost per-entry results"));
    }
    for result in &plan.entry_results {
        if result.recheck != RecheckOutcome::Superseded
            || result.delete != DeleteOutcome::Eligible
            || result.confirmed_absent
        {
            return Err(fail("superseded entry result misclassified"));
        }
    }
    Ok(())
}

fn check_replacement_and_active_coverage_retained() -> Result<(), Box<dyn Error>> {
    let (mut request, key_a, _) = superseded_request()?;
    let replacement = request
        .replacement_key
        .clone()
        .ok_or_else(|| fail("missing replacement"))?;
    request.active_keys = vec![key_a.clone()];
    let plan = decide_cleanup(&request)?;
    if plan.deletable_ids != vec![12] {
        return Err(fail("active coverage was not retained"));
    }
    request.candidates.push(CandidateEntry {
        repository_id: 1,
        cache_id: 13,
        ref_name: "refs/heads/main".to_owned(),
        key: replacement,
        version: "v1".to_owned(),
        size_in_bytes: 4096,
    });
    request.candidate_ids = vec![11, 12, 13];
    let plan = decide_cleanup(&request)?;
    if plan.deletable_ids != vec![12] {
        return Err(fail("replacement itself was deleted"));
    }
    let retained: Vec<u64> = plan
        .entry_results
        .iter()
        .filter(|result| result.delete == DeleteOutcome::Retained)
        .map(|result| result.cache_id)
        .collect();
    if retained != vec![11, 13] {
        return Err(fail("retained set omitted active coverage or replacement"));
    }
    Ok(())
}

fn check_out_of_scope_entries_retained() -> Result<(), Box<dyn Error>> {
    let (mut request, _, _) = superseded_request()?;
    let mut foreign_family = fixture_generation("coverage-foreign", 0)?;
    foreign_family.family = Gc1Family::Tools;
    let mut foreign_group = fixture_generation("coverage-foreign", 0)?;
    foreign_group.group_digest = Digest::parse(&digest_b3(b"gc1-t27-other-group"))?;
    let mut foreign_compat = fixture_generation("coverage-foreign", 0)?;
    foreign_compat.compatibility_digest = Digest::parse(&digest_b3(b"gc1-t27-other-compat"))?;
    for (cache_id, generation) in [
        (13, foreign_family),
        (14, foreign_group),
        (15, foreign_compat),
    ] {
        request.candidates.push(CandidateEntry {
            repository_id: 1,
            cache_id,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: 4096,
        });
    }
    request.candidate_ids = vec![11, 12, 13, 14, 15];
    let plan = decide_cleanup(&request)?;
    if plan.deletable_ids != vec![11, 12] {
        return Err(fail("out-of-scope entry was retired"));
    }
    let retained: Vec<u64> = plan
        .entry_results
        .iter()
        .filter(|result| result.delete == DeleteOutcome::Retained)
        .map(|result| result.cache_id)
        .collect();
    if retained != vec![13, 14, 15] {
        return Err(fail("out-of-scope entries missed retention"));
    }
    for result in &plan.entry_results {
        if result.recheck == RecheckOutcome::Superseded && result.delete != DeleteOutcome::Eligible
        {
            return Err(fail("superseded entry result misclassified"));
        }
    }
    Ok(())
}

fn check_repair_keeps_predecessor() -> Result<(), Box<dyn Error>> {
    let (request, bad_entry) = repair_request()?;
    let plan = decide_cleanup(&request)?;
    if plan.deletable_ids != vec![12] {
        return Err(fail("repair deleted more than the bad exact ID"));
    }
    if plan
        .entry_results
        .iter()
        .find(|result| result.cache_id == 11)
        .is_none_or(|result| {
            result.recheck != RecheckOutcome::Retained || result.delete != DeleteOutcome::Retained
        })
    {
        return Err(fail("repair did not retain the usable predecessor"));
    }
    if !confirm_absent(12, &[11]) {
        return Err(fail("confirmed absence not observed after deletion"));
    }
    if confirm_absent(12, &[11, 12]) {
        return Err(fail("present entry reported as absent"));
    }
    let released = release_charges(&bad_entry, true)?;
    if released.bytes != bad_entry.size_in_bytes || released.slots != 1 {
        return Err(fail("confirmed absence released wrong charges"));
    }
    if admit_repair_generation(12, &[11], 1)? != 1 {
        return Err(fail("repair generation not admitted after deletion"));
    }
    Ok(())
}

fn check_missing_readback_is_unverified() -> Result<(), Box<dyn Error>> {
    let unverified = ReadbackState::Unverified {
        reason: "readback-not-available".to_owned(),
    };
    if require_reproducibly_corrupt(&unverified).is_ok() {
        return Err(fail("unverified readback treated as corruption"));
    }
    let verified = ReadbackState::Verified;
    if require_reproducibly_corrupt(&verified).is_ok() {
        return Err(fail("usable readback treated as corruption"));
    }
    let (mut request, _) = repair_request()?;
    request.bad_readback = None;
    if decide_cleanup(&request).is_ok() {
        return Err(fail("missing readback authorized a repair"));
    }
    request.bad_readback = Some(unverified);
    if decide_cleanup(&request).is_ok() {
        return Err(fail("unverified readback authorized a repair"));
    }
    Ok(())
}

#[test]
fn gc1_t27_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_superseded_retires_only_superseded()?;
    check_replacement_and_active_coverage_retained()?;
    check_out_of_scope_entries_retained()?;
    check_repair_keeps_predecessor()?;
    check_missing_readback_is_unverified()?;
    Ok(())
}

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

fn check_rejects_bad_scope() -> Result<(), Box<dyn Error>> {
    let (mut request, _, _) = superseded_request()?;
    request.repository_id = 0;
    must_fail!(decide_cleanup(&request), "zero repository");
    let (mut request, _, _) = superseded_request()?;
    request.ownership_repository_id = 2;
    must_fail!(decide_cleanup(&request), "foreign ownership");
    let (mut request, _, _) = superseded_request()?;
    request.candidates[0].repository_id = 2;
    must_fail!(decide_cleanup(&request), "foreign candidate");
    let (mut request, _, _) = superseded_request()?;
    request.intent_current = false;
    must_fail!(decide_cleanup(&request), "stale intent");
    let (mut request, _, _) = superseded_request()?;
    request.intent_repository_id = 2;
    must_fail!(decide_cleanup(&request), "intent repository drift");
    Ok(())
}

fn check_rejects_bad_ids() -> Result<(), Box<dyn Error>> {
    if CLEANUP_SCHEMA != 1 {
        return Err(fail("cleanup schema drifted"));
    }
    must_fail!(validate_candidate_ids(&[]), "empty IDs");
    must_fail!(validate_candidate_ids(&[0]), "zero ID");
    must_fail!(validate_candidate_ids(&[12, 11]), "unsorted IDs");
    must_fail!(validate_candidate_ids(&[11, 11]), "duplicate IDs");
    let oversized: Vec<u64> = (1..=u64::try_from(MAX_CLEANUP_CANDIDATES)? + 1).collect();
    must_fail!(validate_candidate_ids(&oversized), "unbounded IDs");
    let (mut request, _, _) = superseded_request()?;
    request.candidates = vec![request.candidates[0].clone()];
    request.candidate_ids = vec![11, 99];
    must_fail!(decide_cleanup(&request), "unknown ID");
    let (mut request, _, key_b) = superseded_request()?;
    let _ = key_b;
    "not-a-key".clone_into(&mut request.candidates[0].key);
    must_fail!(decide_cleanup(&request), "malformed candidate key");
    let (mut request, _, _) = superseded_request()?;
    request.candidates[0].cache_id = 0;
    must_fail!(decide_cleanup(&request), "zero candidate ID");
    let (mut request, _, _) = superseded_request()?;
    request.candidates[0].ref_name = "r".repeat(MAX_CLEANUP_TEXT_BYTES + 1);
    must_fail!(decide_cleanup(&request), "overlong candidate text");
    Ok(())
}

fn check_rejects_bad_replacement() -> Result<(), Box<dyn Error>> {
    let (mut request, _, _) = superseded_request()?;
    request.replacement_key = None;
    must_fail!(decide_cleanup(&request), "missing replacement");
    let (mut request, _, _) = superseded_request()?;
    request.replacement_key = Some("not-a-key".to_owned());
    must_fail!(decide_cleanup(&request), "malformed replacement");
    let (mut request, key_a, key_b) = superseded_request()?;
    let _ = key_b;
    request.replacement_key = Some(key_a.clone());
    request.active_keys = vec![key_a];
    request.candidate_ids = vec![11];
    must_fail!(decide_cleanup(&request), "nothing superseded");
    let (mut request, _, _) = superseded_request()?;
    request.active_keys = vec!["not-a-key".to_owned()];
    must_fail!(decide_cleanup(&request), "malformed active key");
    Ok(())
}

fn check_rejects_bad_repair() -> Result<(), Box<dyn Error>> {
    let (mut request, _) = repair_request()?;
    request.repair_authorized = false;
    must_fail!(decide_cleanup(&request), "unauthorized repair");
    let (mut request, _) = repair_request()?;
    request.repair_bad_cache_id = Some(99);
    must_fail!(decide_cleanup(&request), "repair target not queued");
    let (mut request, _) = repair_request()?;
    request.retained_good_key = None;
    must_fail!(decide_cleanup(&request), "missing predecessor");
    let (mut request, _) = repair_request()?;
    request.retained_good_key = Some("not-a-key".to_owned());
    must_fail!(decide_cleanup(&request), "malformed predecessor");
    let (mut request, _) = repair_request()?;
    let bad_key = request.candidates[1].key.clone();
    request.retained_good_key = Some(bad_key);
    must_fail!(decide_cleanup(&request), "predecessor is target");
    let (mut request, _) = repair_request()?;
    let unstable = fixture_generation("coverage-b", 2)?;
    request.retained_good_key = Some(unstable.cache_key());
    request.candidates.push(fixture_entry(13, &unstable));
    must_fail!(decide_cleanup(&request), "unstable predecessor");
    let (mut request, _) = repair_request()?;
    request.candidates.retain(|entry| entry.cache_id != 11);
    must_fail!(decide_cleanup(&request), "absent predecessor");
    let (mut request, _) = repair_request()?;
    request.bad_readback = Some(ReadbackState::Corrupt {
        evidence: String::new(),
    });
    must_fail!(decide_cleanup(&request), "empty corrupt evidence");
    Ok(())
}

fn check_rejects_unconfirmed_release() -> Result<(), Box<dyn Error>> {
    let (_, bad_entry) = repair_request()?;
    must_fail!(release_charges(&bad_entry, false), "unconfirmed release");
    must_fail!(
        admit_repair_generation(12, &[11, 12], 1),
        "admit before deletion"
    );
    must_fail!(admit_repair_generation(0, &[11], 1), "zero repair target");
    must_fail!(
        admit_repair_generation(12, &[11], 0),
        "zero repair revision"
    );
    Ok(())
}

#[test]
fn gc1_t27_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_scope()?;
    check_rejects_bad_ids()?;
    check_rejects_bad_replacement()?;
    check_rejects_bad_repair()?;
    check_rejects_unconfirmed_release()?;
    Ok(())
}
