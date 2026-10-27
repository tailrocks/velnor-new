// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::cache_context::{
    MeasuredU64, ObservationGapCode, ObservationGapV1,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;

fn internal_inventory(problem: &str) -> OrchestratorError {
    OrchestratorError::Inventory {
        problem: problem.to_owned(),
    }
}

pub const INVENTORY_SCHEMA: u32 = 1;

pub const MAX_INVENTORY_ENTRIES: usize = 100_000;

pub const MAX_INVENTORY_JSON_BYTES: usize = 4 * 1024 * 1024;

pub const MAX_CACHE_REF_BYTES: usize = 512;

pub const MAX_CACHE_KEY_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryTupleV1 {
    pub repository_id: u64,
    pub cache_id: u64,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub key: String,
    pub version: String,
    pub size_in_bytes: u64,
    pub created_at: String,
    pub last_accessed_at: String,
}

impl EntryTupleV1 {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.repository_id == 0 {
            return Err(internal_inventory("entry_repository_id_not_positive"));
        }
        if self.cache_id == 0 {
            return Err(internal_inventory("entry_cache_id_not_positive"));
        }
        require_text("entry.ref", &self.ref_name, MAX_CACHE_REF_BYTES)?;
        require_text("entry.key", &self.key, MAX_CACHE_KEY_BYTES)?;
        require_text("entry.version", &self.version, MAX_CACHE_REF_BYTES)?;
        require_text("entry.created_at", &self.created_at, MAX_CACHE_REF_BYTES)?;
        require_text(
            "entry.last_accessed_at",
            &self.last_accessed_at,
            MAX_CACHE_REF_BYTES,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryV1 {
    pub schema: u32,
    pub repository_id: u64,
    pub captured_at: String,
    pub entries: Vec<EntryTupleV1>,
    pub complete: bool,
    pub pages_observed: u64,
    pub unknown_reason: Option<ObservationGapV1>,
    pub sum_stored_bytes: MeasuredU64,
}

impl InventoryV1 {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != INVENTORY_SCHEMA {
            return Err(internal_inventory("inventory_schema_unsupported"));
        }
        if self.repository_id == 0 {
            return Err(internal_inventory("inventory_repository_id_not_positive"));
        }
        require_text(
            "inventory.captured_at",
            &self.captured_at,
            MAX_CACHE_REF_BYTES,
        )?;
        if self.entries.len() > MAX_INVENTORY_ENTRIES {
            return Err(internal_inventory("inventory_entry_limit"));
        }
        let mut previous: Option<u64> = None;
        let mut running: u64 = 0;
        for entry in &self.entries {
            entry.validate()?;
            if entry.repository_id != self.repository_id {
                return Err(internal_inventory("inventory_entry_repository_mismatch"));
            }
            if let Some(last) = previous
                && entry.cache_id <= last
            {
                return Err(internal_inventory("inventory_entries_not_sorted_unique"));
            }
            previous = Some(entry.cache_id);
            running = running
                .checked_add(entry.size_in_bytes)
                .ok_or_else(|| internal_inventory("inventory_sum_overflow"))?;
        }
        if let Some(reason) = &self.unknown_reason {
            reason.validate().map_err(internal_contract)?;
        }
        self.sum_stored_bytes
            .validate()
            .map_err(internal_contract)?;
        if self.complete {
            if self.pages_observed == 0 {
                return Err(internal_inventory("inventory_complete_without_pages"));
            }
            if self.unknown_reason.is_some() {
                return Err(internal_inventory("inventory_complete_with_unknown"));
            }
            match self.sum_stored_bytes.value {
                Some(total) if total == running => {}
                _ => return Err(internal_inventory("inventory_sum_mismatch")),
            }
        } else {
            if self.unknown_reason.is_none() {
                return Err(internal_inventory("inventory_incomplete_without_reason"));
            }
            if self.sum_stored_bytes.value.is_some() {
                return Err(internal_inventory("inventory_unknown_claimed_known"));
            }
        }
        Ok(())
    }
}

pub fn unknown_inventory_gap(detail: &str) -> Result<ObservationGapV1, OrchestratorError> {
    if detail.is_empty() || detail.bytes().any(|byte| byte == 0) {
        return Err(internal_inventory("inventory_gap_detail"));
    }
    Ok(ObservationGapV1 {
        code: ObservationGapCode::MissingSession,
        detail: detail.to_owned(),
        affected_recipe_ids: Vec::new(),
        evidence_ref: "actions-cache-inventory".to_owned(),
    })
}

#[must_use]
pub fn measured_bytes(value: u64) -> MeasuredU64 {
    MeasuredU64 {
        value: Some(value),
        unknown_reason: None,
    }
}

pub fn unknown_bytes(detail: &str) -> Result<MeasuredU64, OrchestratorError> {
    Ok(MeasuredU64 {
        value: None,
        unknown_reason: Some(unknown_inventory_gap(detail)?),
    })
}

pub fn build_inventory(
    repository_id: u64,
    captured_at: &str,
    mut entries: Vec<EntryTupleV1>,
    pages_observed: u64,
) -> Result<InventoryV1, OrchestratorError> {
    entries.sort_by_key(|entry| entry.cache_id);
    let inventory = InventoryV1 {
        schema: INVENTORY_SCHEMA,
        repository_id,
        captured_at: captured_at.to_owned(),
        entries,
        complete: true,
        pages_observed,
        unknown_reason: None,
        sum_stored_bytes: MeasuredU64 {
            value: None,
            unknown_reason: None,
        },
    };
    let mut running: u64 = 0;
    for entry in &inventory.entries {
        entry.validate()?;
        if entry.repository_id != repository_id {
            return Err(internal_inventory("inventory_entry_repository_mismatch"));
        }
        running = running
            .checked_add(entry.size_in_bytes)
            .ok_or_else(|| internal_inventory("inventory_sum_overflow"))?;
    }
    let inventory = InventoryV1 {
        sum_stored_bytes: measured_bytes(running),
        ..inventory
    };
    inventory.validate()?;
    Ok(inventory)
}

pub fn mark_inventory_unknown(
    repository_id: u64,
    captured_at: &str,
    pages_observed: u64,
    detail: &str,
) -> Result<InventoryV1, OrchestratorError> {
    let inventory = InventoryV1 {
        schema: INVENTORY_SCHEMA,
        repository_id,
        captured_at: captured_at.to_owned(),
        entries: Vec::new(),
        complete: false,
        pages_observed,
        unknown_reason: Some(unknown_inventory_gap(detail)?),
        sum_stored_bytes: unknown_bytes(detail)?,
    };
    inventory.validate()?;
    Ok(inventory)
}

pub fn inventory_canonical_bytes(value: &InventoryV1) -> Result<Vec<u8>, OrchestratorError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(internal_contract)
}

pub fn parse_inventory_bytes(bytes: &[u8]) -> Result<InventoryV1, OrchestratorError> {
    let value =
        parse_strict_json_bytes(bytes, MAX_INVENTORY_JSON_BYTES).map_err(internal_contract)?;
    let inventory: InventoryV1 = serde_json::from_value(value)
        .map_err(|error| internal_inventory(&format!("inventory_json:{error}")))?;
    inventory.validate()?;
    Ok(inventory)
}

pub fn inventory_digest(inventory: &InventoryV1) -> Result<Digest, OrchestratorError> {
    inventory.validate()?;
    let bytes = canonical_json_bytes(inventory).map_err(internal_contract)?;
    Ok(digest_b3_typed(&bytes))
}

fn require_text(field: &str, value: &str, limit: usize) -> Result<(), OrchestratorError> {
    if value.is_empty() || value.len() > limit || value.bytes().any(|byte| byte == 0) {
        return Err(internal_inventory(&format!("{field}_invalid")));
    }
    Ok(())
}
