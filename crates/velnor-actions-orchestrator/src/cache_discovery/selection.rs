// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::workflow::cache_identity::{GC1_NAMESPACE, GenerationId};

use super::types::{
    DISCOVERY_SCHEMA, DiscoveryCandidate, DiscoveryDecision, DiscoveryError, DiscoveryRequest,
    DiscoveryV1, EntryProvenance, MAX_ALLOWED_REFS, SelectableEntry, ShadowState, contract_error,
    discovery_error, require_text,
};

#[must_use]
pub fn primary_prefix(generation: &GenerationId) -> String {
    format!(
        "{GC1_NAMESPACE}-{}-{}-{}-{}",
        generation.family.as_str(),
        generation.group_digest.as_str(),
        generation.compatibility_digest.as_str(),
        generation.coverage_digest.as_str(),
    )
}

fn visible_entries(request: &DiscoveryRequest) -> Vec<&SelectableEntry> {
    let allowed: BTreeSet<&str> = request.allowed_refs.iter().map(String::as_str).collect();
    request
        .entries
        .iter()
        .filter(|entry| {
            entry.repository_id == request.job.repository_id
                && entry.version == request.expected_cache_version
                && allowed.contains(entry.ref_name.as_str())
        })
        .collect()
}

pub fn select_exact(
    request: &DiscoveryRequest,
    entries: &[&SelectableEntry],
) -> Option<DiscoveryCandidate> {
    let exact_key = request.generation.cache_key();
    entries
        .iter()
        .filter(|entry| entry.key == exact_key)
        .min_by_key(|entry| entry.cache_id)
        .map(|entry| DiscoveryCandidate {
            entry: (*entry).clone(),
            generation: request.generation.clone(),
        })
}

pub fn classify_shadow(
    exact_refs: &BTreeSet<String>,
    longer_refs: &BTreeSet<String>,
    current_ref: &str,
) -> ShadowState {
    if exact_refs.len() > 1 {
        return ShadowState::ConflictingVisibleRef;
    }
    if let Some(hit_ref) = exact_refs.iter().next()
        && hit_ref.as_str() != current_ref
        && longer_refs.contains(current_ref)
    {
        return ShadowState::ConflictingVisibleRef;
    }
    if longer_refs.len() > 1 {
        return ShadowState::ConflictingVisibleRef;
    }
    ShadowState::Clear
}

fn shadow_inputs(
    request: &DiscoveryRequest,
    entries: &[&SelectableEntry],
) -> (BTreeSet<String>, BTreeSet<String>) {
    let exact_key = request.generation.cache_key();
    let prefix = primary_prefix(&request.generation);
    let mut exact_refs = BTreeSet::new();
    let mut longer_refs = BTreeSet::new();
    for entry in entries {
        if entry.key == exact_key {
            exact_refs.insert(entry.ref_name.clone());
        } else if entry.key.starts_with(&prefix) {
            longer_refs.insert(entry.ref_name.clone());
        }
    }
    (exact_refs, longer_refs)
}

fn untrusted_selectable(entries: &[&SelectableEntry]) -> Option<u64> {
    entries
        .iter()
        .filter(|entry| entry.provenance != EntryProvenance::Trusted)
        .min_by_key(|entry| entry.cache_id)
        .map(|entry| entry.cache_id)
}

fn provenance_scope<'a>(
    request: &DiscoveryRequest,
    entries: &[&'a SelectableEntry],
) -> Vec<&'a SelectableEntry> {
    let exact_key = request.generation.cache_key();
    entries
        .iter()
        .copied()
        .filter(|entry| {
            if entry.key == exact_key {
                return true;
            }
            entry.parsed_generation().is_some_and(|generation| {
                generation.family == request.generation.family
                    && generation.group_digest == request.generation.group_digest
                    && generation.compatibility_digest == request.generation.compatibility_digest
            })
        })
        .collect()
}

pub fn select_predecessor(
    request: &DiscoveryRequest,
    entries: &[&SelectableEntry],
) -> Option<DiscoveryCandidate> {
    let exact_key = request.generation.cache_key();
    let mut ranked: Vec<(&SelectableEntry, GenerationId, usize)> = entries
        .iter()
        .filter_map(|entry| {
            let generation = entry.parsed_generation()?;
            if entry.key == exact_key {
                return None;
            }
            if generation.family != request.generation.family {
                return None;
            }
            if generation.group_digest != request.generation.group_digest {
                return None;
            }
            if generation.compatibility_digest != request.generation.compatibility_digest {
                return None;
            }
            let rank = request
                .active_coverage_ids
                .iter()
                .position(|active| active == generation.coverage_digest.as_str())
                .unwrap_or(usize::MAX);
            Some((*entry, generation, rank))
        })
        .collect();
    ranked.sort_by(|left, right| {
        left.2
            .cmp(&right.2)
            .then_with(|| right.0.created_at.cmp(&left.0.created_at))
            .then_with(|| right.0.cache_id.cmp(&left.0.cache_id))
    });
    ranked
        .into_iter()
        .next()
        .map(|(entry, generation, _)| DiscoveryCandidate {
            entry: entry.clone(),
            generation,
        })
}

fn build_record(
    request: &DiscoveryRequest,
    exact_candidate: Option<DiscoveryCandidate>,
    predecessor_candidate: Option<DiscoveryCandidate>,
    shadow_state: ShadowState,
    decision: DiscoveryDecision,
    reason: String,
) -> Result<DiscoveryV1, DiscoveryError> {
    let mut allowed_refs = request.allowed_refs.clone();
    allowed_refs.sort();
    allowed_refs.dedup();
    let record = DiscoveryV1 {
        schema: DISCOVERY_SCHEMA,
        job: request.job.clone(),
        generation: request.generation.clone(),
        expected_cache_version: request.expected_cache_version.clone(),
        allowed_refs,
        inventory_digest: request.inventory_digest.clone(),
        inventory_complete: request.inventory_complete,
        exact_candidate,
        predecessor_candidate,
        shadow_state,
        decision,
        reason,
    };
    record.validate()?;
    Ok(record)
}

pub fn decide_discovery(request: &DiscoveryRequest) -> Result<DiscoveryV1, DiscoveryError> {
    request
        .job
        .validate()
        .map_err(|error| contract_error(&error))?;
    require_text(
        "discovery.expected_cache_version",
        &request.expected_cache_version,
    )?;
    if request.allowed_refs.is_empty() || request.allowed_refs.len() > MAX_ALLOWED_REFS {
        return Err(discovery_error("discovery_allowed_refs_bound"));
    }
    require_text("discovery.current_ref", &request.current_ref)?;
    let visible = visible_entries(request);
    if request.inventory_complete {
        decide_complete(request, &visible)
    } else {
        decide_degraded(request, &visible)
    }
}

fn decide_degraded(
    request: &DiscoveryRequest,
    visible: &[&SelectableEntry],
) -> Result<DiscoveryV1, DiscoveryError> {
    match select_exact(request, visible) {
        Some(candidate) if candidate.entry.provenance == EntryProvenance::Trusted => build_record(
            request,
            Some(candidate),
            None,
            ShadowState::Unknown,
            DiscoveryDecision::ExactHit,
            "exact_key_with_independent_provenance_without_inventory".to_owned(),
        ),
        _ => build_record(
            request,
            None,
            None,
            ShadowState::Unknown,
            DiscoveryDecision::ColdUnavailable,
            "inventory_unavailable_without_trusted_exact".to_owned(),
        ),
    }
}

fn decide_complete(
    request: &DiscoveryRequest,
    visible: &[&SelectableEntry],
) -> Result<DiscoveryV1, DiscoveryError> {
    let (exact_refs, longer_refs) = shadow_inputs(request, visible);
    let shadow = classify_shadow(&exact_refs, &longer_refs, &request.current_ref);
    let exact = select_exact(request, visible);
    let predecessor = if exact.is_none() {
        select_predecessor(request, visible)
    } else {
        None
    };
    if exact.is_some() || predecessor.is_some() {
        if shadow == ShadowState::ConflictingVisibleRef {
            return build_record(
                request,
                None,
                None,
                shadow,
                DiscoveryDecision::RefusedShadow,
                "conflicting_visible_ref_before_extraction".to_owned(),
            );
        }
        if let Some(cache_id) = untrusted_selectable(&provenance_scope(request, visible)) {
            return build_record(
                request,
                None,
                None,
                shadow,
                DiscoveryDecision::RefusedProvenance,
                format!("untrusted_selectable_entry:{cache_id}"),
            );
        }
    }
    if let Some(candidate) = exact {
        return build_record(
            request,
            Some(candidate),
            None,
            shadow,
            DiscoveryDecision::ExactHit,
            "exact_requested_key_first".to_owned(),
        );
    }
    match predecessor {
        Some(candidate) => build_record(
            request,
            None,
            Some(candidate),
            shadow,
            DiscoveryDecision::PredecessorFallback,
            "single_compatible_retained_predecessor".to_owned(),
        ),
        None => build_record(
            request,
            None,
            None,
            shadow,
            DiscoveryDecision::ColdMissing,
            "no_compatible_retained_entry".to_owned(),
        ),
    }
}

#[cfg(test)]
mod gc1_t42_newest_predecessor {
    use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
    use velnor_actions_contract::workflow::cache_context::JobBindingV1;
    use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

    use super::super::types::{DiscoveryRequest, EntryProvenance, SelectableEntry};
    use super::select_predecessor;

    fn fixture_job() -> Result<JobBindingV1, Box<dyn std::error::Error>> {
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
            plan_digest: Digest::parse(&digest_b3(b"gc1-t42-plan"))?,
        })
    }

    fn fixture_generation() -> Result<GenerationId, Box<dyn std::error::Error>> {
        Ok(GenerationId {
            family: Gc1Family::State,
            group_digest: Digest::parse(&digest_b3(b"gc1-t42-group"))?,
            compatibility_digest: Digest::parse(&digest_b3(b"gc1-t42-compat"))?,
            coverage_digest: Digest::parse(&digest_b3(b"gc1-t42-coverage"))?,
            repair_revision: 0,
        })
    }

    fn predecessor_generation(seed: &[u8]) -> Result<GenerationId, Box<dyn std::error::Error>> {
        let base = fixture_generation()?;
        Ok(GenerationId {
            coverage_digest: Digest::parse(&digest_b3(seed))?,
            ..base
        })
    }

    fn entry(cache_id: u64, key: &str, created_at: &str) -> SelectableEntry {
        SelectableEntry {
            cache_id,
            repository_id: 1,
            ref_name: "refs/heads/main".to_owned(),
            key: key.to_owned(),
            version: "v1".to_owned(),
            size_in_bytes: 512,
            created_at: created_at.to_owned(),
            provenance: EntryProvenance::Trusted,
        }
    }

    fn discovery_request(
        generation: GenerationId,
        entries: Vec<SelectableEntry>,
    ) -> Result<DiscoveryRequest, Box<dyn std::error::Error>> {
        Ok(DiscoveryRequest {
            job: fixture_job()?,
            generation,
            expected_cache_version: "v1".to_owned(),
            allowed_refs: vec!["refs/heads/main".to_owned()],
            current_ref: "refs/heads/main".to_owned(),
            inventory_digest: digest_b3_typed(b"gc1-t42-inventory"),
            inventory_complete: true,
            entries,
            active_coverage_ids: Vec::new(),
        })
    }

    #[test]
    fn gc1_t42_selects_newest_compatible_predecessor() -> Result<(), Box<dyn std::error::Error>> {
        let generation = fixture_generation()?;
        let older = predecessor_generation(b"gc1-t42-older-coverage")?;
        let newer = predecessor_generation(b"gc1-t42-newer-coverage")?;
        let request = discovery_request(
            generation,
            vec![
                entry(7, &older.cache_key(), "2026-10-20T00:00:00Z"),
                entry(9, &newer.cache_key(), "2026-10-24T00:00:00Z"),
            ],
        )?;
        let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
        let predecessor = select_predecessor(&request, &visible)
            .ok_or_else(|| std::io::Error::other("compatible predecessor missed selection"))?;
        if predecessor.entry.cache_id != 9 {
            return Err(std::io::Error::other(
                "predecessor selection kept the oldest entry instead of the newest",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn gc1_t42_prefers_larger_cache_id_on_timestamp_tie() -> Result<(), Box<dyn std::error::Error>>
    {
        let generation = fixture_generation()?;
        let first = predecessor_generation(b"gc1-t42-tie-a-coverage")?;
        let second = predecessor_generation(b"gc1-t42-tie-b-coverage")?;
        let request = discovery_request(
            generation,
            vec![
                entry(7, &first.cache_key(), "2026-10-22T00:00:00Z"),
                entry(9, &second.cache_key(), "2026-10-22T00:00:00Z"),
            ],
        )?;
        let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
        let predecessor = select_predecessor(&request, &visible)
            .ok_or_else(|| std::io::Error::other("tied predecessor missed selection"))?;
        if predecessor.entry.cache_id != 9 {
            return Err(std::io::Error::other(
                "timestamp tie-break kept the smaller cache id instead of the larger",
            )
            .into());
        }
        Ok(())
    }
}
