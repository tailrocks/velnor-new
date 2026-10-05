//! Final aggregate report.
use crate::errors::ContractError;
use crate::ids::{
    artifact_id_for_final, validate_artifact_id, validate_report_id, validate_run_key,
};
use serde::{Deserialize, Serialize};

/// Final aggregate report (`final-<run-key>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalReport {
    /// Report schema version; must be 1.
    pub schema: u32,
    /// Derived final report ID.
    pub report_id: String,
    /// Run key.
    pub run_key: String,
    /// Validated plan ID.
    pub plan_id: String,
    /// Every expected matrix report ID (sorted).
    pub expected_report_ids: Vec<String>,
    /// Every downloaded artifact name (sorted).
    pub downloaded_artifact_ids: Vec<String>,
    /// Required non-matrix job results.
    pub required_job_results: Vec<RequiredJobResult>,
    /// Computed final result.
    pub status: FinalStatus,
    /// Status counts.
    pub counts: FinalCounts,
    /// Cache miss/save-failure reasons observed while merging.
    #[serde(default)]
    pub miss_reasons: Vec<String>,
}
/// One required job conclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredJobResult {
    /// Job ID.
    pub job_id: String,
    /// Job conclusion.
    pub conclusion: JobConclusion,
}
/// Closed job-conclusion vocabulary shared by every merge path.
///
/// The needs channel admits successes, failures, cancellations, and
/// skips; `Neutral` folds as not-run and `Missing` marks declared
/// validators that never reported (failed, never silent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobConclusion {
    /// Validator succeeded.
    Success,
    /// Validator failed.
    Failure,
    /// Validator was cancelled.
    Cancelled,
    /// Validator was skipped.
    Skipped,
    /// Validator finished neutral; never success.
    Neutral,
    /// Declared validator never reported.
    Missing,
}

impl JobConclusion {
    /// Stable conclusion spelling.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Neutral => "neutral",
            Self::Missing => "missing",
        }
    }

    /// Parse a conclusion spelling; unknown tokens fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for spellings outside the vocabulary.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "success" => Ok(Self::Success),
            "failure" => Ok(Self::Failure),
            "cancelled" => Ok(Self::Cancelled),
            "skipped" => Ok(Self::Skipped),
            "neutral" => Ok(Self::Neutral),
            "missing" => Ok(Self::Missing),
            _ => Err(ContractError::identity(
                "conclusion",
                format!("unknown_conclusion:{value}"),
            )),
        }
    }
}
/// Final gate result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalStatus {
    /// Valid plan with zero obligations.
    NoWork,
    /// Every obligation satisfied.
    Passed,
    /// A required task failed.
    Failed,
    /// A required task was cancelled.
    Cancelled,
    /// A required task was blocked (`not_selected`), below cancelled.
    Blocked,
    /// A selected entry lacks a valid report.
    NotRun,
    /// Planning or validation failed.
    PlanningFailed,
}
/// Final status counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalCounts {
    /// Selected count.
    pub selected: u32,
    /// Reused count.
    pub reused: u32,
    /// Executed count.
    pub executed: u32,
    /// Empty-partition count.
    pub empty_partition: u32,
    /// Baseline-covered count.
    pub covered: u32,
    /// Failed count.
    pub failed: u32,
    /// Cancelled count.
    pub cancelled: u32,
    /// Blocked count.
    pub blocked: u32,
    /// Not-run count.
    pub not_run: u32,
}
/// Derive the final report ID `final-<run-key>`.
/// # Errors
pub fn final_report_id_for_run(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("final-{run_key}"))
}
/// Final-report path under `$RUNNER_TEMP` (cache §3).
/// # Errors
pub fn final_report_relpath(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("velnor/{run_key}/final-report.json"))
}
/// Matrix-report path under `$RUNNER_TEMP` (cache §3).
/// # Errors
pub fn matrix_report_relpath(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::ids::validate_matrix_key(matrix_key)?;
    Ok(format!("velnor/{run_key}/{matrix_key}/matrix-report.json"))
}
/// Task-report path under `$RUNNER_TEMP` (task §5).
/// # Errors
pub fn task_report_relpath(
    run_key: &str,
    matrix_key: &str,
    task_report_id: &str,
) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::ids::validate_matrix_key(matrix_key)?;
    crate::ids::validate_task_report_id(task_report_id)?;
    Ok(format!(
        "velnor/{run_key}/{matrix_key}/tasks/{task_report_id}.json"
    ))
}
/// Join a report relpath under `$RUNNER_TEMP` (cache §3).
///
/// Pure path join: rejects an empty temp dir, absolute relpaths, and
/// parent traversal. Writers call this; readers resolve the same way.
/// # Errors
pub fn join_runner_temp(runner_temp: &str, relpath: &str) -> Result<String, ContractError> {
    if runner_temp.trim().is_empty() {
        return Err(ContractError::identity("runner_temp", "empty_temp_dir"));
    }
    if runner_temp.contains("..") {
        return Err(ContractError::identity("runner_temp", "parent_traversal"));
    }
    if relpath.is_empty() || relpath.starts_with('/') || relpath.contains('\\') {
        return Err(ContractError::identity("relpath", "non_relative_path"));
    }
    if relpath.split('/').any(|seg| seg.is_empty() || seg == "..") {
        return Err(ContractError::identity("relpath", "parent_traversal"));
    }
    Ok(format!("{}/{}", runner_temp.trim_end_matches('/'), relpath))
}

/// Validate a final report ID.
/// # Errors
pub fn validate_final_report_id(value: &str) -> Result<(), ContractError> {
    let Some(run_key) = value.strip_prefix("final-") else {
        return Err(ContractError::identity(
            "report_id",
            "malformed_final_report_id",
        ));
    };
    validate_run_key(run_key)
        .map_err(|_| ContractError::identity("report_id", "malformed_final_report_id"))
}
impl FinalReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;
    /// Honest verdict when no plan exists: `planning_failed`, zero counts.
    ///
    /// Job results must arrive sorted by job ID; expected/downloaded lists
    /// stay empty because nothing was scheduled or fetched.
    /// # Errors
    pub fn without_plan(
        run_key: &str,
        required_job_results: Vec<RequiredJobResult>,
    ) -> Result<Self, ContractError> {
        validate_run_key(run_key)?;
        let mut jobs = required_job_results;
        jobs.sort_by(|left, right| left.job_id.cmp(&right.job_id));
        Ok(Self {
            schema: Self::SCHEMA,
            report_id: final_report_id_for_run(run_key)?,
            run_key: run_key.to_owned(),
            plan_id: crate::ids::plan_id_for_run(run_key)?,
            expected_report_ids: Vec::new(),
            downloaded_artifact_ids: Vec::new(),
            required_job_results: jobs,
            status: FinalStatus::PlanningFailed,
            counts: FinalCounts {
                selected: 0,
                reused: 0,
                executed: 0,
                empty_partition: 0,
                covered: 0,
                failed: 0,
                cancelled: 0,
                blocked: 0,
                not_run: 0,
            },
            miss_reasons: Vec::new(),
        })
    }
    /// Validate schema, derived IDs, sorting, and artifact names.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_final_report_id(&self.report_id)?;
        validate_run_key(&self.run_key)?;
        crate::ids::validate_plan_id(&self.plan_id)?;
        if !is_sorted(&self.expected_report_ids) || !is_sorted(&self.downloaded_artifact_ids) {
            return Err(ContractError::identity("final_report", "must_be_sorted"));
        }
        for report_id in &self.expected_report_ids {
            validate_report_id(report_id)?;
        }
        for artifact_id in &self.downloaded_artifact_ids {
            validate_artifact_id(artifact_id)?;
        }
        for reason in &self.miss_reasons {
            crate::cachekey::validate_miss_reason(reason)?;
        }
        Ok(())
    }
    /// Derive the expected final artifact name for this report.
    /// # Errors
    pub fn artifact_id(&self) -> Result<String, ContractError> {
        artifact_id_for_final(&self.run_key)
    }
}
/// Check a schema-1 version marker.
fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}
/// Check a string list is sorted.
fn is_sorted(list: &[String]) -> bool {
    list.windows(2).all(|pair| pair[0] <= pair[1])
}

#[cfg(test)]
mod deny_unknown_fields_tests {
    //! Unknown-key rejection for the workflow wire types.
    //!
    //! Homed here because `report.rs` is at the file-size gate; the
    //! task/matrix types live there, `RequiredJobResult` lives here.

    use serde::de::DeserializeOwned;
    use serde::ser::Serialize;

    use super::{JobConclusion, RequiredJobResult};
    use crate::workflow::plan::WorkflowEvent;
    use crate::workflow::platform::{
        PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason,
    };
    use crate::workflow::report::{
        CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
        TaskReport, TaskStatus,
    };
    use crate::workflow::trust::Trust;

    /// Round-trip the value, then prove one unknown key rejects.
    fn rejects_unknown(value: &impl Serialize, typed: fn(serde_json::Value) -> bool) {
        let mut wire = serde_json::to_value(value).expect("serialize");
        assert!(typed(wire.clone()), "valid wire must parse");
        wire["velnor_unknown_probe"] = serde_json::json!(1);
        assert!(!typed(wire), "unknown key must reject");
    }

    /// Parse probe for one wire type.
    fn parses<T: DeserializeOwned>(wire: serde_json::Value) -> bool {
        serde_json::from_value::<T>(wire).is_ok()
    }

    #[test]
    fn task_wire_rejects_unknown_keys() {
        let task = TaskReport {
            schema: TaskReport::SCHEMA,
            task_report_id: "task-local-m-0123456789abcdef-0123456789abcdef".to_owned(),
            run_key: "local".to_owned(),
            event: WorkflowEvent::Push,
            trust: Trust::Trusted,
            matrix_id: "stack:rust|task:internal/plan/default".to_owned(),
            matrix_key: "m-0123456789abcdef".to_owned(),
            task_id: "internal/plan/default".to_owned(),
            task_digest: "b3-00".to_owned(),
            status: TaskStatus::Failed,
            not_selected_reason: None,
            cache: CacheOutcome {
                layer: CacheLayer::Task,
                key: String::new(),
                result: CacheResult::NotAttempted,
                miss_reason: None,
            },
            platform_binding: PlatformBinding::Unavailable {
                planned_platform_id: crate::canonical::digest_b3(b"planned-platform"),
                runner_environment: PlatformRunnerEnvironment::Unknown,
                reason: PlatformUnavailableReason::ObservationNotRecorded,
            },
            exit_code: 1,
            duration_ms: None,
            outputs: Vec::new(),
            lane: None,
            queue: None,
            partition: None,
            reason: None,
            timing: None,
        };
        rejects_unknown(&task.cache, parses::<CacheOutcome>);
        rejects_unknown(&task, parses::<TaskReport>);
    }

    #[test]
    fn matrix_wire_rejects_unknown_keys() {
        let entry = MatrixTaskEntry {
            task_report_id: "task-local-m-0123456789abcdef-0123456789abcdef".to_owned(),
            task_id: "internal/plan/default".to_owned(),
            status: TaskStatus::Failed,
            exit_code: 1,
        };
        let matrix = MatrixReport {
            schema: MatrixReport::SCHEMA,
            report_id: "report-local-m-0123456789abcdef".to_owned(),
            run_key: "local".to_owned(),
            matrix_id: "stack:rust|task:internal/plan/default".to_owned(),
            matrix_key: "m-0123456789abcdef".to_owned(),
            status: MatrixStatus::Failed,
            expected_task_ids: Vec::new(),
            task_report_ids: Vec::new(),
            tasks: vec![entry],
            selected: 1,
            reused: 0,
            executed: 0,
            empty_partition: 0,
            not_selected: 0,
            failed: 1,
            cancelled: 0,
        };
        rejects_unknown(&matrix.tasks[0], parses::<MatrixTaskEntry>);
        rejects_unknown(&matrix, parses::<MatrixReport>);
        rejects_unknown(
            &RequiredJobResult {
                job_id: "plan".to_owned(),
                conclusion: JobConclusion::Success,
            },
            parses::<RequiredJobResult>,
        );
    }
}
