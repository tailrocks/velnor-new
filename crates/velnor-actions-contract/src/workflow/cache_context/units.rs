// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cachekey::ToolchainInputs;
use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{
    check_schema, parse_strict_record, require_nonempty, require_sorted_unique,
};
use crate::workflow::task_execution_manifest::TaskExecutionManifestEntryV1;

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
        if compare_utc_timestamps(&self.started_at, &self.finished_at)
            == std::cmp::Ordering::Greater
        {
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

    /// Specified encoding covered by [`UnitResultV1::missing_report_digest`]
    /// when a unit has no staged `TaskReport`.
    pub const NO_TASK_REPORT_DIGEST_INPUT: &str = "velnor-gc1/no-task-report/v1";

    /// Report digest: `b3(canonical staged TaskReport)`.
    ///
    /// The caller passes the canonical bytes of the staged task report; the
    /// digest is the raw BLAKE3 hash of those bytes.
    #[must_use]
    pub fn report_digest_from_canonical_bytes(bytes: &[u8]) -> Digest {
        digest_b3_typed(bytes)
    }

    /// Report digest for a unit with no staged report: the digest covers the
    /// [`UnitResultV1::NO_TASK_REPORT_DIGEST_INPUT`] absence encoding.
    #[must_use]
    pub fn missing_report_digest() -> Digest {
        digest_b3_typed(Self::NO_TASK_REPORT_DIGEST_INPUT.as_bytes())
    }

    /// Recipe digest mapped from actual manifest entry fields: the BLAKE3
    /// hash of the canonical JSON of `(task_digest, toolchain_inputs,
    /// report_helper_version)` taken from the task execution manifest entry.
    /// It identifies the recipe (what the task runs and with which
    /// toolchain) independently of the runtime inputs covered by
    /// [`UnitResultV1::effective_input_digest_from_manifest_entry`].
    pub fn recipe_digest_from_manifest_entry(
        entry: &TaskExecutionManifestEntryV1,
    ) -> Result<Digest, ContractError> {
        let inputs = RecipeDigestInputs {
            task_digest: entry.task_digest.as_str(),
            toolchain_inputs: &entry.toolchain_inputs,
            report_helper_version: entry.report_helper_version.as_str(),
        };
        Ok(digest_b3_typed(&canonical_json_bytes(&inputs)?))
    }

    /// Argv digest mapped from the actual manifest entry field: the BLAKE3
    /// hash of the canonical JSON of the entry `argv` vector.
    pub fn argv_digest_from_manifest_entry(
        entry: &TaskExecutionManifestEntryV1,
    ) -> Result<Digest, ContractError> {
        Ok(digest_b3_typed(&canonical_json_bytes(&entry.argv)?))
    }

    /// Effective-input digest mapped from actual manifest entry fields: the
    /// BLAKE3 hash of the canonical JSON of `(env, matrix_id, matrix_key,
    /// matrix_max_parallel)` taken from the task execution manifest entry.
    pub fn effective_input_digest_from_manifest_entry(
        entry: &TaskExecutionManifestEntryV1,
    ) -> Result<Digest, ContractError> {
        let inputs = EffectiveInputDigestInputs {
            env: &entry.env,
            matrix_id: entry.matrix_id.as_str(),
            matrix_key: entry.matrix_key.as_str(),
            matrix_max_parallel: entry.matrix_max_parallel,
        };
        Ok(digest_b3_typed(&canonical_json_bytes(&inputs)?))
    }
}

fn split_utc_timestamp(value: &str) -> Option<(&str, &str)> {
    let body = value.strip_suffix('Z')?;
    match body.split_once('.') {
        Some((head, fraction)) => Some((head, fraction)),
        None => Some((body, "")),
    }
}

fn compare_fraction(left: &str, right: &str) -> std::cmp::Ordering {
    let width = left.len().max(right.len());
    format!("{left:0<width$}").cmp(&format!("{right:0<width$}"))
}

fn compare_utc_timestamps(started: &str, finished: &str) -> std::cmp::Ordering {
    match (split_utc_timestamp(started), split_utc_timestamp(finished)) {
        (Some((start_head, start_frac)), Some((finish_head, finish_frac))) => start_head
            .cmp(finish_head)
            .then_with(|| compare_fraction(start_frac, finish_frac)),
        _ => started.cmp(finished),
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

// Group-ledger producer digest constructors.
//
// Every digest emitted in a `UnitLedgerV1` derives from real producer inputs
// through the construction documented on each associated function below.
// Nothing here hashes an empty or synthetic placeholder: when a staged input
// is absent, the digest covers the specified absence-encoding constant named
// on that function, so absence is explicit evidence rather than an invented
// value. The constructors are associated functions (not free functions) so
// they stay reachable through the already-exported ledger types.

#[derive(Serialize)]
pub(crate) struct RecipeDigestInputs<'a> {
    pub(crate) task_digest: &'a str,
    pub(crate) toolchain_inputs: &'a ToolchainInputs,
    pub(crate) report_helper_version: &'a str,
}

#[derive(Serialize)]
pub(crate) struct EffectiveInputDigestInputs<'a> {
    pub(crate) env: &'a BTreeMap<String, String>,
    pub(crate) matrix_id: &'a str,
    pub(crate) matrix_key: &'a str,
    pub(crate) matrix_max_parallel: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct GroupDigestInputs<'a> {
    pub(crate) group_key: &'a str,
    pub(crate) plan_digest: &'a Digest,
}

#[cfg(test)]
#[path = "units_gc1_t53.rs"]
mod gc1_t53;
