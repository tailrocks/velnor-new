// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_identity::GenerationId;

pub const CLEANUP_SCHEMA: u32 = 1;

pub const MAX_CLEANUP_TEXT_BYTES: usize = 512;

pub const MAX_CLEANUP_CANDIDATES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cache_cleanup: {problem}")]
pub struct CleanupError {
    pub problem: String,
}

fn cleanup_error(problem: &str) -> CleanupError {
    CleanupError {
        problem: problem.to_owned(),
    }
}

fn require_text(field: &str, value: &str) -> Result<(), CleanupError> {
    if value.is_empty()
        || value.len() > MAX_CLEANUP_TEXT_BYTES
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(cleanup_error(&format!("{field}_invalid")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEntry {
    pub repository_id: u64,
    pub cache_id: u64,
    pub ref_name: String,
    pub key: String,
    pub version: String,
    pub size_in_bytes: u64,
}

impl CandidateEntry {
    pub fn validate(&self) -> Result<(), CleanupError> {
        if self.repository_id == 0 {
            return Err(cleanup_error("cleanup_entry_repository_invalid"));
        }
        if self.cache_id == 0 {
            return Err(cleanup_error("cleanup_entry_id_invalid"));
        }
        require_text("cleanup.entry.ref_name", &self.ref_name)?;
        require_text("cleanup.entry.key", &self.key)?;
        require_text("cleanup.entry.version", &self.version)?;
        GenerationId::parse_key(&self.key)
            .map_err(|_| cleanup_error("cleanup_entry_key_malformed"))?;
        Ok(())
    }
}

pub fn validate_candidate_ids(ids: &[u64]) -> Result<(), CleanupError> {
    if ids.is_empty() {
        return Err(cleanup_error("cleanup_candidates_empty"));
    }
    if ids.len() > MAX_CLEANUP_CANDIDATES {
        return Err(cleanup_error("cleanup_candidates_unbounded"));
    }
    if ids.contains(&0) {
        return Err(cleanup_error("cleanup_candidate_id_invalid"));
    }
    for window in ids.windows(2) {
        if window[0] >= window[1] {
            return Err(cleanup_error("cleanup_candidate_ids_unsorted_duplicate"));
        }
    }
    Ok(())
}

pub fn validate_replacement_key(key: &str) -> Result<GenerationId, CleanupError> {
    require_text("cleanup.replacement_key", key)?;
    GenerationId::parse_key(key).map_err(|_| cleanup_error("cleanup_replacement_malformed"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode {
    SupersededRetirement,
    CorruptRepair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadbackState {
    Verified,
    Corrupt { evidence: String },
    Unverified { reason: String },
}

pub fn require_reproducibly_corrupt(state: &ReadbackState) -> Result<String, CleanupError> {
    match state {
        ReadbackState::Corrupt { evidence } => {
            require_text("cleanup.corrupt_evidence", evidence)?;
            Ok(evidence.clone())
        }
        ReadbackState::Verified => Err(cleanup_error("cleanup_usable_not_corrupt")),
        ReadbackState::Unverified { .. } => Err(cleanup_error("cleanup_unverified_not_corrupt")),
    }
}

pub struct CleanupRequest {
    pub repository_id: u64,
    pub ownership_repository_id: u64,
    pub mode: CleanupMode,
    pub candidates: Vec<CandidateEntry>,
    pub candidate_ids: Vec<u64>,
    pub intent_current: bool,
    pub intent_repository_id: u64,
    pub replacement_key: Option<String>,
    pub active_keys: Vec<String>,
    pub repair_authorized: bool,
    pub repair_bad_cache_id: Option<u64>,
    pub retained_good_key: Option<String>,
    pub bad_readback: Option<ReadbackState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecheckOutcome {
    Superseded,
    ApprovedRepair,
    Retained,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    Eligible,
    Retained,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryResult {
    pub cache_id: u64,
    pub recheck: RecheckOutcome,
    pub delete: DeleteOutcome,
    pub confirmed_absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlan {
    pub mode: CleanupMode,
    pub deletable_ids: Vec<u64>,
    pub entry_results: Vec<EntryResult>,
}

fn find_candidate(candidates: &[CandidateEntry], cache_id: u64) -> Option<&CandidateEntry> {
    candidates.iter().find(|entry| entry.cache_id == cache_id)
}

fn validate_active_keys(active_keys: &[String]) -> Result<(), CleanupError> {
    for key in active_keys {
        require_text("cleanup.active_key", key)?;
        GenerationId::parse_key(key).map_err(|_| cleanup_error("cleanup_active_key_malformed"))?;
    }
    Ok(())
}

fn validate_request_scope(request: &CleanupRequest) -> Result<(), CleanupError> {
    if request.repository_id == 0 {
        return Err(cleanup_error("cleanup_repository_invalid"));
    }
    if request.ownership_repository_id != request.repository_id {
        return Err(cleanup_error("cleanup_ownership_foreign"));
    }
    if !request.intent_current {
        return Err(cleanup_error("cleanup_intent_stale"));
    }
    if request.intent_repository_id != request.repository_id {
        return Err(cleanup_error("cleanup_intent_repository_mismatch"));
    }
    Ok(())
}

fn validate_observed_candidates(request: &CleanupRequest) -> Result<(), CleanupError> {
    validate_candidate_ids(&request.candidate_ids)?;
    if request.candidates.is_empty() {
        return Err(cleanup_error("cleanup_candidates_empty"));
    }
    for candidate in &request.candidates {
        candidate.validate()?;
        if candidate.repository_id != request.repository_id {
            return Err(cleanup_error("cleanup_candidate_foreign"));
        }
    }
    for id in &request.candidate_ids {
        if find_candidate(&request.candidates, *id).is_none() {
            return Err(cleanup_error("cleanup_candidate_unknown"));
        }
    }
    validate_active_keys(&request.active_keys)
}

fn is_active(request: &CleanupRequest, entry: &CandidateEntry) -> bool {
    request.active_keys.iter().any(|key| key == &entry.key)
}

fn in_replacement_scope(entry: &CandidateEntry, replacement: &GenerationId) -> bool {
    GenerationId::parse_key(&entry.key).is_ok_and(|generation| {
        generation.family == replacement.family
            && generation.group_digest == replacement.group_digest
            && generation.compatibility_digest == replacement.compatibility_digest
    })
}

fn plan_superseded(request: &CleanupRequest) -> Result<CleanupPlan, CleanupError> {
    let replacement = request
        .replacement_key
        .as_deref()
        .ok_or_else(|| cleanup_error("cleanup_replacement_missing"))?;
    let replacement_generation = validate_replacement_key(replacement)?;
    let mut deletable_ids = Vec::new();
    let mut entry_results = Vec::new();
    for id in &request.candidate_ids {
        let entry = find_candidate(&request.candidates, *id)
            .ok_or_else(|| cleanup_error("cleanup_candidate_unknown"))?;
        if entry.key == replacement
            || is_active(request, entry)
            || !in_replacement_scope(entry, &replacement_generation)
        {
            entry_results.push(EntryResult {
                cache_id: *id,
                recheck: RecheckOutcome::Retained,
                delete: DeleteOutcome::Retained,
                confirmed_absent: false,
            });
        } else {
            deletable_ids.push(*id);
            entry_results.push(EntryResult {
                cache_id: *id,
                recheck: RecheckOutcome::Superseded,
                delete: DeleteOutcome::Eligible,
                confirmed_absent: false,
            });
        }
    }
    if deletable_ids.is_empty() {
        return Err(cleanup_error("cleanup_nothing_superseded"));
    }
    Ok(CleanupPlan {
        mode: request.mode,
        deletable_ids,
        entry_results,
    })
}

fn validate_repair_target(request: &CleanupRequest) -> Result<u64, CleanupError> {
    if !request.repair_authorized {
        return Err(cleanup_error("cleanup_repair_unauthorized"));
    }
    let bad_id = request
        .repair_bad_cache_id
        .ok_or_else(|| cleanup_error("cleanup_repair_target_missing"))?;
    if !request.candidate_ids.contains(&bad_id) {
        return Err(cleanup_error("cleanup_repair_target_not_queued"));
    }
    let bad_entry = find_candidate(&request.candidates, bad_id)
        .ok_or_else(|| cleanup_error("cleanup_candidate_unknown"))?;
    let retained = request
        .retained_good_key
        .as_deref()
        .ok_or_else(|| cleanup_error("cleanup_repair_predecessor_missing"))?;
    require_text("cleanup.retained_good_key", retained)?;
    let retained_generation = GenerationId::parse_key(retained)
        .map_err(|_| cleanup_error("cleanup_repair_predecessor_malformed"))?;
    if retained == bad_entry.key {
        return Err(cleanup_error("cleanup_repair_predecessor_is_target"));
    }
    if retained_generation.repair_revision != 0 {
        return Err(cleanup_error("cleanup_repair_predecessor_not_stable"));
    }
    if !request.candidates.iter().any(|entry| entry.key == retained) {
        return Err(cleanup_error("cleanup_repair_predecessor_absent"));
    }
    let readback = request
        .bad_readback
        .as_ref()
        .ok_or_else(|| cleanup_error("cleanup_unverified_not_corrupt"))?;
    require_reproducibly_corrupt(readback)?;
    Ok(bad_id)
}

fn plan_repair(request: &CleanupRequest) -> Result<CleanupPlan, CleanupError> {
    let bad_id = validate_repair_target(request)?;
    let mut deletable_ids = Vec::new();
    let mut entry_results = Vec::new();
    for id in &request.candidate_ids {
        if *id == bad_id {
            deletable_ids.push(*id);
            entry_results.push(EntryResult {
                cache_id: *id,
                recheck: RecheckOutcome::ApprovedRepair,
                delete: DeleteOutcome::Eligible,
                confirmed_absent: false,
            });
        } else {
            entry_results.push(EntryResult {
                cache_id: *id,
                recheck: RecheckOutcome::Retained,
                delete: DeleteOutcome::Retained,
                confirmed_absent: false,
            });
        }
    }
    Ok(CleanupPlan {
        mode: request.mode,
        deletable_ids,
        entry_results,
    })
}

pub fn decide_cleanup(request: &CleanupRequest) -> Result<CleanupPlan, CleanupError> {
    validate_request_scope(request)?;
    validate_observed_candidates(request)?;
    match request.mode {
        CleanupMode::SupersededRetirement => plan_superseded(request),
        CleanupMode::CorruptRepair => plan_repair(request),
    }
}

#[must_use]
pub fn confirm_absent(cache_id: u64, post_cleanup_ids: &[u64]) -> bool {
    !post_cleanup_ids.contains(&cache_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedCharges {
    pub bytes: u64,
    pub slots: u64,
}

pub fn release_charges(
    entry: &CandidateEntry,
    confirmed_absent: bool,
) -> Result<ReleasedCharges, CleanupError> {
    entry.validate()?;
    if !confirmed_absent {
        return Err(cleanup_error("cleanup_release_unconfirmed"));
    }
    Ok(ReleasedCharges {
        bytes: entry.size_in_bytes,
        slots: 1,
    })
}

pub fn admit_repair_generation(
    bad_cache_id: u64,
    post_cleanup_ids: &[u64],
    repair_revision: u64,
) -> Result<u64, CleanupError> {
    if bad_cache_id == 0 {
        return Err(cleanup_error("cleanup_repair_target_missing"));
    }
    if !confirm_absent(bad_cache_id, post_cleanup_ids) {
        return Err(cleanup_error("cleanup_repair_before_deletion"));
    }
    if repair_revision == 0 {
        return Err(cleanup_error("cleanup_repair_revision_invalid"));
    }
    Ok(repair_revision)
}
