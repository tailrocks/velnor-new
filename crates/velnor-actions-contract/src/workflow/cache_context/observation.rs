// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::canonical_json_bytes;
use crate::errors::ContractError;
use crate::workflow::cache_identity::{parse_strict_record, require_nonempty};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationGapCode {
    UnknownNativeCoverage,
    UnobservedNetworkAccess,
    IncompleteAttribution,
    MissingSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationGapV1 {
    pub code: ObservationGapCode,
    pub detail: String,
    pub affected_recipe_ids: Vec<String>,
    pub evidence_ref: String,
}

impl ObservationGapV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("observation_gap.detail", &self.detail)?;
        require_nonempty("observation_gap.evidence_ref", &self.evidence_ref)?;
        for recipe_id in &self.affected_recipe_ids {
            require_nonempty("observation_gap.affected_recipe_ids", recipe_id)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredU64 {
    pub value: Option<u64>,
    pub unknown_reason: Option<ObservationGapV1>,
}

impl MeasuredU64 {
    pub fn validate(&self) -> Result<(), ContractError> {
        match (&self.value, &self.unknown_reason) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => {
                return Err(ContractError::identity(
                    "measured_u64",
                    "exactly_one_of_value_or_unknown_reason",
                ));
            }
        }
        if let Some(reason) = &self.unknown_reason {
            reason.validate()?;
        }
        Ok(())
    }

    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let record: Self = parse_strict_record(text)?;
        record.validate()?;
        Ok(record)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}
