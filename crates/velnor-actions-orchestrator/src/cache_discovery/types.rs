// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::GenerationId;

pub const DISCOVERY_SCHEMA: u32 = 1;

pub const MAX_DISCOVERY_JSON_BYTES: usize = 64 * 1024;

pub const MAX_DISCOVERY_TEXT_BYTES: usize = 512;

pub const MAX_ALLOWED_REFS: usize = 1024;

pub const MAX_REASON_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("discovery: {problem}")]
pub struct DiscoveryError {
    pub problem: String,
}

pub(super) fn discovery_error(problem: &str) -> DiscoveryError {
    DiscoveryError {
        problem: problem.to_owned(),
    }
}

pub(super) fn contract_error(error: &velnor_actions_contract::ContractError) -> DiscoveryError {
    discovery_error(&error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryProvenance {
    Trusted,
    Untrusted,
    Ambiguous,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectableEntry {
    pub cache_id: u64,
    pub repository_id: u64,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub key: String,
    pub version: String,
    pub size_in_bytes: u64,
    pub created_at: String,
    pub provenance: EntryProvenance,
}

impl SelectableEntry {
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        if self.cache_id == 0 {
            return Err(discovery_error("entry_cache_id_not_positive"));
        }
        if self.repository_id == 0 {
            return Err(discovery_error("entry_repository_id_not_positive"));
        }
        require_text("entry.ref", &self.ref_name)?;
        require_text("entry.key", &self.key)?;
        require_text("entry.version", &self.version)?;
        require_text("entry.created_at", &self.created_at)?;
        GenerationId::parse_key(&self.key).map_err(|_| discovery_error("entry_key_malformed"))?;
        Ok(())
    }

    pub fn parsed_generation(&self) -> Option<GenerationId> {
        GenerationId::parse_key(&self.key).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryCandidate {
    pub entry: SelectableEntry,
    pub generation: GenerationId,
}

impl DiscoveryCandidate {
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        self.entry.validate()?;
        let parsed = self
            .entry
            .parsed_generation()
            .ok_or_else(|| discovery_error("candidate_key_malformed"))?;
        if parsed != self.generation {
            return Err(discovery_error("candidate_generation_mismatch"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowState {
    Clear,
    ConflictingVisibleRef,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryDecision {
    ExactHit,
    PredecessorFallback,
    ColdMissing,
    ColdUnavailable,
    RefusedShadow,
    RefusedProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub expected_cache_version: String,
    pub allowed_refs: Vec<String>,
    pub inventory_digest: Digest,
    pub inventory_complete: bool,
    pub exact_candidate: Option<DiscoveryCandidate>,
    pub predecessor_candidate: Option<DiscoveryCandidate>,
    pub shadow_state: ShadowState,
    pub decision: DiscoveryDecision,
    pub reason: String,
}

impl DiscoveryV1 {
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        if self.schema != DISCOVERY_SCHEMA {
            return Err(discovery_error("discovery_schema_unsupported"));
        }
        self.job
            .validate()
            .map_err(|error| contract_error(&error))?;
        GenerationId::parse_key(&self.generation.cache_key())
            .map_err(|_| discovery_error("discovery_generation_malformed"))?;
        require_text(
            "discovery.expected_cache_version",
            &self.expected_cache_version,
        )?;
        if self.allowed_refs.is_empty() || self.allowed_refs.len() > MAX_ALLOWED_REFS {
            return Err(discovery_error("discovery_allowed_refs_bound"));
        }
        for ref_name in &self.allowed_refs {
            require_text("discovery.allowed_refs", ref_name)?;
        }
        if self.allowed_refs.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(discovery_error("discovery_allowed_refs_not_sorted_unique"));
        }
        if self.reason.is_empty() || self.reason.len() > MAX_REASON_BYTES {
            return Err(discovery_error("discovery_reason_bound"));
        }
        match (&self.exact_candidate, &self.predecessor_candidate) {
            (Some(exact), None) => {
                exact.validate()?;
            }
            (None, Some(predecessor)) => {
                predecessor.validate()?;
            }
            (None, None) => {}
            (Some(_), Some(_)) => {
                return Err(discovery_error("discovery_two_candidates"));
            }
        }
        match self.decision {
            DiscoveryDecision::ExactHit => {
                if self.exact_candidate.is_none() {
                    return Err(discovery_error("discovery_decision_without_exact"));
                }
            }
            DiscoveryDecision::PredecessorFallback => {
                if self.predecessor_candidate.is_none() {
                    return Err(discovery_error("discovery_decision_without_predecessor"));
                }
            }
            DiscoveryDecision::ColdMissing
            | DiscoveryDecision::ColdUnavailable
            | DiscoveryDecision::RefusedShadow
            | DiscoveryDecision::RefusedProvenance => {
                if self.exact_candidate.is_some() || self.predecessor_candidate.is_some() {
                    return Err(discovery_error("discovery_cold_with_candidate"));
                }
            }
        }
        if !self.inventory_complete {
            if self.predecessor_candidate.is_some() {
                return Err(discovery_error("discovery_predecessor_without_inventory"));
            }
            if self.shadow_state != ShadowState::Unknown {
                return Err(discovery_error("discovery_shadow_without_inventory"));
            }
            if !matches!(
                self.decision,
                DiscoveryDecision::ExactHit | DiscoveryDecision::ColdUnavailable
            ) {
                return Err(discovery_error("discovery_decision_without_inventory"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRequest {
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub expected_cache_version: String,
    pub allowed_refs: Vec<String>,
    pub current_ref: String,
    pub inventory_digest: Digest,
    pub inventory_complete: bool,
    pub entries: Vec<SelectableEntry>,
    pub active_coverage_ids: Vec<String>,
}

pub(super) fn require_text(field: &str, value: &str) -> Result<(), DiscoveryError> {
    if value.is_empty()
        || value.len() > MAX_DISCOVERY_TEXT_BYTES
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(discovery_error(&format!("{field}_invalid")));
    }
    Ok(())
}
