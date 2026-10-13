use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{canonical_json_bytes, canonical_json_str};
use velnor_actions_contract_workflow::{
    FINAL_JSON_FILENAME, FinalStatus, MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, matrix_json_bytes,
    plan_json_bytes,
};

use crate::OrchestratorError;
use crate::decisions::plan_artifact_dir;
use crate::internal::{PlanResponse, check_schema, internal, internal_contract};
use crate::plan_output_limits::{PlanOutputMode, check_plan_outputs};

/// Canonical `plan`/`matrix` outputs for `$GITHUB_OUTPUT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutputs {
    /// Canonical matrix JSON (single line).
    pub matrix: String,
    /// Plan ID for step outputs (WF-4.15).
    pub plan_id: String,
    /// Run key for step outputs (WF-4.15).
    pub run_key: String,
    /// Comma-wrapped covered task IDs (empty when none covered).
    pub covered_tasks: String,
    /// Aggregate UTF-16 byte size of values promoted by this output mode.
    pub job_outputs_utf16_bytes: usize,
}

/// Split one `plan-v1` response into canonical `$GITHUB_OUTPUT` values.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for malformed responses.
pub fn plan_outputs(
    response_json: &str,
    mode: PlanOutputMode,
) -> Result<PlanOutputs, OrchestratorError> {
    let response: PlanResponse =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(response.schema)?;
    let mut outputs = PlanOutputs {
        matrix: canonical_json_str(&response.matrix).map_err(internal_contract)?,
        plan_id: response.plan.plan_id.clone(),
        run_key: response.plan.run_key.clone(),
        covered_tasks: crate::covered_tasks::CoveredTasks::for_plan(&response.plan).encode(),
        job_outputs_utf16_bytes: 0,
    };
    outputs.job_outputs_utf16_bytes = check_plan_outputs(
        mode,
        response.matrix.include.len(),
        &outputs.promoted_job_outputs(mode),
    )?;
    Ok(outputs)
}

impl PlanOutputs {
    /// Required named outputs written by the plan step, in stable order.
    #[must_use]
    pub fn step_outputs(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("matrix", &self.matrix),
            ("plan_id", &self.plan_id),
            ("run_key", &self.run_key),
            (crate::COVERED_TASKS_OUTPUT, &self.covered_tasks),
        ]
    }

    /// Output records promoted to job outputs by this workflow path.
    #[must_use]
    pub fn promoted_job_outputs(&self, mode: PlanOutputMode) -> Vec<(&'static str, &str)> {
        match mode {
            PlanOutputMode::Static => {
                vec![(crate::COVERED_TASKS_OUTPUT, &self.covered_tasks)]
            }
            PlanOutputMode::DynamicMatrix => self.step_outputs(),
        }
    }
}

/// Publish `plan.json` + `matrix.json` for the plan artifact.
///
/// Contract §4 fixes these files under `<velnor-dir>/<run-key>/`, the
/// directory uploaded by the plan job.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for malformed responses and
/// [`OrchestratorError::Io`] for unwritable directories.
pub fn publish_plan_files(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let response: PlanResponse =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(response.schema)?;
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(&response, velnor_dir, &dir)?;
    Ok(dir)
}

fn write_plan_files(
    response: &PlanResponse,
    velnor_dir: &Path,
    dir: &Path,
) -> Result<(), OrchestratorError> {
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, dir)?;
    let plan = plan_json_bytes(&response.plan).map_err(internal_contract)?;
    let matrix = matrix_json_bytes(&response.matrix).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(&dir.join(PLAN_JSON_FILENAME), &plan, "plan_artifact")?;
    crate::exclusive_write::write_exclusive(
        &dir.join(MATRIX_JSON_FILENAME),
        &matrix,
        "plan_artifact",
    )?;
    if let Some(manifest) = response.baseline_manifest.as_ref() {
        let bytes = canonical_json_bytes(manifest).map_err(internal_contract)?;
        crate::exclusive_write::write_exclusive(
            &dir.join(crate::baseline_publish::BASELINE_FILENAME),
            &bytes,
            "plan_artifact",
        )?;
    }
    Ok(())
}

/// Publish `final-report.json` for the final artifact.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for malformed responses and
/// [`OrchestratorError::Io`] for unwritable directories.
pub fn publish_final_report(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let report: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(report.schema)?;
    let dir = plan_artifact_dir(velnor_dir, &report.run_key)?;
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, &dir)?;
    let bytes = canonical_json_bytes(&report).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(
        &dir.join(FINAL_JSON_FILENAME),
        &bytes,
        "plan_artifact",
    )?;
    Ok(dir)
}

/// True when one `merge-v1` response is a passing verdict.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for malformed responses.
pub fn merge_passed(response_json: &str) -> Result<bool, OrchestratorError> {
    #[derive(Debug, Deserialize)]
    struct Verdict {
        status: FinalStatus,
    }
    let verdict: Verdict =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    Ok(matches!(
        verdict.status,
        FinalStatus::Passed | FinalStatus::NoWork
    ))
}

fn artifact_anchor(velnor_dir: &Path) -> Result<&Path, OrchestratorError> {
    velnor_dir
        .parent()
        .ok_or_else(|| internal("missing_dir_anchor"))
}
