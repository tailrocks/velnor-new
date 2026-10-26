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
            .then_with(|| left.0.created_at.cmp(&right.0.created_at))
            .then_with(|| left.0.cache_id.cmp(&right.0.cache_id))
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
        if let Some(cache_id) = untrusted_selectable(visible) {
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
