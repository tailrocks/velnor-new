// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{
    check_schema, parse_strict_record, require_nonempty, require_sorted_unique,
};

use super::JobBindingV1;
use super::validation::validate_rfc3339_utc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheRestoreOutcome {
    Hit,
    Partial,
    Cold,
    Denied,
    Corrupt,
    Unavailable,
}

impl CacheRestoreOutcome {
    #[must_use]
    pub const fn requires_cold_execution(self) -> bool {
        matches!(
            self,
            Self::Cold | Self::Denied | Self::Corrupt | Self::Unavailable
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBranch {
    Reader,
    Writer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitOutcome {
    Executed,
    CacheHit,
    NeedsPrepare,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitResultV1 {
    pub unit_id: String,
    pub recipe_id: Digest,
    pub task_identity: String,
    pub argv_digest: Digest,
    pub effective_input_digest: Digest,
    pub outcome: UnitOutcome,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub finished_at: String,
    pub report_digest: Digest,
}

impl UnitResultV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("unit_result.unit_id", &self.unit_id)?;
        require_nonempty("unit_result.task_identity", &self.task_identity)?;
        match self.outcome {
            UnitOutcome::Executed | UnitOutcome::Failed => {
                if self.exit_code.is_none() {
                    return Err(ContractError::identity(
                        "unit_result.exit_code",
                        "executed_result_requires_exit_code",
                    ));
                }
            }
            UnitOutcome::CacheHit | UnitOutcome::NeedsPrepare | UnitOutcome::Cancelled => {
                if self.exit_code.is_some() {
                    return Err(ContractError::identity(
                        "unit_result.exit_code",
                        "unexecuted_result_must_not_carry_exit_code",
                    ));
                }
            }
        }
        validate_rfc3339_utc("unit_result.started_at", &self.started_at)?;
        validate_rfc3339_utc("unit_result.finished_at", &self.finished_at)?;
        if self.finished_at < self.started_at {
            return Err(ContractError::identity(
                "unit_result.finished_at",
                "finished_before_started",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub const fn is_successful_completion(&self) -> bool {
        matches!(self.outcome, UnitOutcome::Executed) && matches!(self.exit_code, Some(0))
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitLedgerV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub group_digest: Digest,
    pub execution_branch: ExecutionBranch,
    pub selected_unit_ids: Vec<String>,
    pub ordered_results: Vec<UnitResultV1>,
    pub observation_digest: Digest,
    pub source_binding_digest: Digest,
}

impl UnitLedgerV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "unit_ledger.schema")?;
        self.job.validate()?;
        require_sorted_unique("unit_ledger.selected_unit_ids", &self.selected_unit_ids)?;
        if self.selected_unit_ids.is_empty() {
            return Err(ContractError::identity(
                "unit_ledger.selected_unit_ids",
                "empty_selected_units",
            ));
        }
        if self.ordered_results.len() != self.selected_unit_ids.len() {
            return Err(ContractError::identity(
                "unit_ledger.ordered_results",
                "coverage_mismatch",
            ));
        }
        for (result, expected) in self
            .ordered_results
            .iter()
            .zip(self.selected_unit_ids.iter())
        {
            result.validate()?;
            if result.unit_id.as_str() != expected.as_str() {
                return Err(ContractError::identity(
                    "unit_ledger.ordered_results",
                    "order_mismatch",
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn successful_completion_unit_ids(&self) -> Vec<&str> {
        self.ordered_results
            .iter()
            .filter(|result| result.is_successful_completion())
            .map(|result| result.unit_id.as_str())
            .collect()
    }

    pub fn ledger_digest(&self) -> Result<Digest, ContractError> {
        self.validate()?;
        Ok(digest_b3_typed(&canonical_json_bytes(self)?))
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
