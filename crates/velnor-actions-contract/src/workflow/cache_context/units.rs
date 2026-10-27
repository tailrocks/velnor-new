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
struct RecipeDigestInputs<'a> {
    task_digest: &'a str,
    toolchain_inputs: &'a ToolchainInputs,
    report_helper_version: &'a str,
}

#[derive(Serialize)]
struct EffectiveInputDigestInputs<'a> {
    env: &'a BTreeMap<String, String>,
    matrix_id: &'a str,
    matrix_key: &'a str,
    matrix_max_parallel: Option<u32>,
}

#[derive(Serialize)]
struct GroupDigestInputs<'a> {
    group_key: &'a str,
    plan_digest: &'a Digest,
}

impl UnitLedgerV1 {
    /// Canonical plan.json bytes digest: `b3(canonical plan.json)`.
    ///
    /// The caller passes the canonical bytes of the downloaded plan document
    /// (the byte form produced by `plan_json_bytes` / staged publication),
    /// and the digest is the raw BLAKE3 hash of those bytes.
    #[must_use]
    pub fn plan_digest_from_canonical_bytes(bytes: &[u8]) -> Digest {
        digest_b3_typed(bytes)
    }

    /// Group digest: `b3(canonical (group_key, plan_digest))`.
    ///
    /// `group_key` is the executing job key with the `-read`/`-write`
    /// branch suffix removed; `plan_digest` is the
    /// [`UnitLedgerV1::plan_digest_from_canonical_bytes`] digest of the same
    /// plan the ledger was emitted against.
    pub fn group_digest_for_group(
        group_key: &str,
        plan_digest: &Digest,
    ) -> Result<Digest, ContractError> {
        if group_key.is_empty() {
            return Err(ContractError::identity(
                "group_ledger.group_key",
                "unknown_required_identity",
            ));
        }
        let inputs = GroupDigestInputs {
            group_key,
            plan_digest,
        };
        Ok(digest_b3_typed(&canonical_json_bytes(&inputs)?))
    }

    /// Specified absence encoding covered per unit by
    /// [`UnitLedgerV1::observation_digest_for_sidecars`] when no observation
    /// sidecar was staged for that unit.
    pub const ABSENT_OBSERVATION_SIDECAR_INPUT: &str = "velnor-gc1/no-observation-sidecar/v1";

    /// Specified absence encoding covered per unit by
    /// [`UnitLedgerV1::source_binding_digest_for_sidecars`] when no
    /// source-binding sidecar was staged for that unit. No producer stages
    /// source-binding sidecars today, so this encoding is what honest emit
    /// produces until one does.
    pub const ABSENT_SOURCE_BINDING_SIDECAR_INPUT: &str = "velnor-gc1/no-source-binding-sidecar/v1";

    /// Combined sidecar digest over the ordered units: each unit contributes
    /// the BLAKE3 hash of its staged sidecar bytes (already canonical) or,
    /// when the unit staged none, of the caller-supplied absence-encoding
    /// constant; the set digest is the BLAKE3 hash of the canonical JSON
    /// array of those per-unit digest strings, in unit order.
    pub fn sidecar_set_digest(
        sidecars: &[Option<&[u8]>],
        absent_input: &str,
    ) -> Result<Digest, ContractError> {
        if absent_input.is_empty() {
            return Err(ContractError::identity(
                "group_ledger.absent_sidecar_input",
                "unknown_required_identity",
            ));
        }
        let mut per_unit = Vec::with_capacity(sidecars.len());
        for sidecar in sidecars {
            let input = sidecar.unwrap_or_else(|| absent_input.as_bytes());
            per_unit.push(digest_b3_typed(input).as_str().to_owned());
        }
        Ok(digest_b3_typed(&canonical_json_bytes(&per_unit)?))
    }

    /// Observation digest from staged unit-observation sidecars, in unit
    /// order: `sidecar_set_digest` with the
    /// [`UnitLedgerV1::ABSENT_OBSERVATION_SIDECAR_INPUT`] absence encoding.
    pub fn observation_digest_for_sidecars(
        sidecars: &[Option<&[u8]>],
    ) -> Result<Digest, ContractError> {
        Self::sidecar_set_digest(sidecars, Self::ABSENT_OBSERVATION_SIDECAR_INPUT)
    }

    /// Source-binding digest from staged source-binding sidecars, in unit
    /// order: `sidecar_set_digest` with the
    /// [`UnitLedgerV1::ABSENT_SOURCE_BINDING_SIDECAR_INPUT`] absence
    /// encoding.
    pub fn source_binding_digest_for_sidecars(
        sidecars: &[Option<&[u8]>],
    ) -> Result<Digest, ContractError> {
        Self::sidecar_set_digest(sidecars, Self::ABSENT_SOURCE_BINDING_SIDECAR_INPUT)
    }
}

// gc1-t53 covers the group-ledger producer contract: zero numerics validate,
// and every digest constructor is deterministic and matches its documented
// construction through an independent canonical-JSON recomputation.
#[cfg(test)]
mod gc1_t53 {
    use super::*;

    fn zero_numeric_job() -> JobBindingV1 {
        JobBindingV1 {
            schema: 1,
            repository_id: 0,
            repository: "tailrocks/velnor-new".to_owned(),
            run_id: 4242,
            run_attempt: 3,
            job_id: 0,
            job_key: "group-write".to_owned(),
            event: "push".to_owned(),
            ref_name: "refs/heads/main".to_owned(),
            workflow_sha: "ab".repeat(20),
            source_sha: "cd".repeat(20),
            plan_digest: digest_b3_typed(b"gc1-t53-plan"),
        }
    }

    #[test]
    fn zero_numerics_validate() -> Result<(), ContractError> {
        zero_numeric_job().validate()?;
        Ok(())
    }

    #[test]
    fn run_identity_still_positive() {
        let mut job = zero_numeric_job();
        job.run_id = 0;
        assert!(job.validate().is_err(), "run_id zero still rejected");
        let mut job = zero_numeric_job();
        job.run_attempt = 0;
        assert!(job.validate().is_err(), "run_attempt zero still rejected");
    }

    fn manifest_entry() -> TaskExecutionManifestEntryV1 {
        TaskExecutionManifestEntryV1 {
            task_id: "task-1".to_owned(),
            execution_digest: "velnor-gc1/execution/v1".to_owned(),
            task_digest: digest_b3_typed(b"gc1-t53-task").as_str().to_owned(),
            toolchain_inputs: ToolchainInputs {
                tools: vec!["rust@1.89.0".to_owned()],
                components: vec!["clippy".to_owned()],
                compile_driver: "cargo".to_owned(),
                test_runner: "nextest".to_owned(),
            },
            argv: vec!["run".to_owned(), "--locked".to_owned()],
            env: BTreeMap::from([("KEY".to_owned(), "value".to_owned())]),
            matrix_id: "matrix-1".to_owned(),
            matrix_key: "key-1".to_owned(),
            report_helper_version: "0.1.57".to_owned(),
            matrix_max_parallel: Some(4),
        }
    }

    #[test]
    fn plan_digest_covers_raw_bytes() {
        let bytes = br#"{"schema":1,"plan":"gc1-t53"}"#;
        assert_eq!(
            UnitLedgerV1::plan_digest_from_canonical_bytes(bytes).as_str(),
            digest_b3_typed(bytes).as_str(),
            "plan digest is b3 of the canonical plan bytes",
        );
    }

    #[test]
    fn report_digest_vectors() {
        let bytes = br#"{"schema":1,"report":"gc1-t53"}"#;
        assert_eq!(
            UnitResultV1::report_digest_from_canonical_bytes(bytes).as_str(),
            digest_b3_typed(bytes).as_str(),
        );
        assert_eq!(
            UnitResultV1::missing_report_digest().as_str(),
            digest_b3_typed(UnitResultV1::NO_TASK_REPORT_DIGEST_INPUT.as_bytes()).as_str(),
            "missing report covers the specified absence encoding",
        );
        assert_ne!(
            UnitResultV1::missing_report_digest().as_str(),
            UnitResultV1::report_digest_from_canonical_bytes(b"").as_str(),
            "absence is never digest(empty)",
        );
    }

    #[test]
    fn recipe_argv_input_map_manifest_fields() -> Result<(), ContractError> {
        let entry = manifest_entry();
        let recipe = UnitResultV1::recipe_digest_from_manifest_entry(&entry)?;
        let rebuilt = RecipeDigestInputs {
            task_digest: entry.task_digest.as_str(),
            toolchain_inputs: &entry.toolchain_inputs,
            report_helper_version: entry.report_helper_version.as_str(),
        };
        assert_eq!(
            recipe.as_str(),
            digest_b3_typed(&canonical_json_bytes(&rebuilt)?).as_str(),
        );
        let argv = UnitResultV1::argv_digest_from_manifest_entry(&entry)?;
        assert_eq!(
            argv.as_str(),
            digest_b3_typed(&canonical_json_bytes(&entry.argv)?).as_str(),
        );
        let input = UnitResultV1::effective_input_digest_from_manifest_entry(&entry)?;
        let rebuilt_input = EffectiveInputDigestInputs {
            env: &entry.env,
            matrix_id: entry.matrix_id.as_str(),
            matrix_key: entry.matrix_key.as_str(),
            matrix_max_parallel: entry.matrix_max_parallel,
        };
        assert_eq!(
            input.as_str(),
            digest_b3_typed(&canonical_json_bytes(&rebuilt_input)?).as_str(),
        );
        let mut changed = entry.clone();
        changed.argv.push("--offline".to_owned());
        assert_ne!(
            UnitResultV1::argv_digest_from_manifest_entry(&changed)?.as_str(),
            argv.as_str(),
            "argv digest tracks the real argv",
        );
        Ok(())
    }

    #[test]
    fn group_digest_binds_group_and_plan() -> Result<(), ContractError> {
        let plan = UnitLedgerV1::plan_digest_from_canonical_bytes(b"gc1-t53-plan");
        let left = UnitLedgerV1::group_digest_for_group("my-group", &plan)?;
        let documented = GroupDigestInputs {
            group_key: "my-group",
            plan_digest: &plan,
        };
        assert_eq!(
            left.as_str(),
            digest_b3_typed(&canonical_json_bytes(&documented)?).as_str(),
        );
        assert_ne!(
            UnitLedgerV1::group_digest_for_group("other-group", &plan)?.as_str(),
            left.as_str(),
        );
        assert!(
            UnitLedgerV1::group_digest_for_group("", &plan).is_err(),
            "empty group key rejected"
        );
        Ok(())
    }

    #[test]
    fn sidecar_digests_cover_bytes_or_absence() -> Result<(), ContractError> {
        let staged = br#"{"session":"gc1-t53"}"#;
        let observed = UnitLedgerV1::observation_digest_for_sidecars(&[Some(&staged[..]), None])?;
        let per_unit = vec![
            digest_b3_typed(staged).as_str().to_owned(),
            digest_b3_typed(UnitLedgerV1::ABSENT_OBSERVATION_SIDECAR_INPUT.as_bytes())
                .as_str()
                .to_owned(),
        ];
        assert_eq!(
            observed.as_str(),
            digest_b3_typed(&canonical_json_bytes(&per_unit)?).as_str(),
        );
        let binding = UnitLedgerV1::source_binding_digest_for_sidecars(&[None, None])?;
        assert_ne!(
            observed.as_str(),
            binding.as_str(),
            "observation and source-binding absence encodings differ",
        );
        assert!(
            UnitLedgerV1::sidecar_set_digest(&[None], "").is_err(),
            "empty absence encoding rejected"
        );
        Ok(())
    }
}
