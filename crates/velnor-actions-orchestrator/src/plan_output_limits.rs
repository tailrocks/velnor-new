//! Fail-closed limits for plan artifacts and job outputs.

use crate::{
    OrchestratorError,
    internal::{PlanResponse, internal, internal_contract},
};
use velnor_actions_contract::{
    QualificationCacheAdmission, QualificationCacheDirective, canonical_json_str,
};

/// Validated plan values emitted to `$GITHUB_OUTPUT` and promoted to the job.
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
    /// Validated qualification namespace, empty outside Qualification.
    pub qualification_campaign: String,
    /// Validated qualification phase, empty outside Qualification.
    pub qualification_phase: String,
    /// Whether this run may restore isolated qualification caches.
    pub qualification_cache_enabled: bool,
    /// Legacy global writer gate; writes require the typed per-layer directives.
    pub qualification_cache_write: bool,
    /// Canonical typed cache directive map, empty outside Qualification.
    pub qualification_cache_directives: String,
    /// Aggregate UTF-16 byte size of values promoted by this output mode.
    pub job_outputs_utf16_bytes: usize,
}

/// Split one `plan-v1` response into canonical output records.
/// # Errors
pub fn plan_outputs(
    response_json: &str,
    mode: PlanOutputMode,
) -> Result<PlanOutputs, OrchestratorError> {
    plan_outputs_with_admission(response_json, mode, None)
}

/// Split one plan response after admitting its staged predecessor evidence.
/// # Errors
pub fn plan_outputs_with_admission(
    response_json: &str,
    mode: PlanOutputMode,
    admission: Option<&QualificationCacheAdmission>,
) -> Result<PlanOutputs, OrchestratorError> {
    let response = PlanResponse::parse(response_json)?;
    let qualification = response.plan.qualification.as_ref();
    let directive = QualificationCacheDirective::for_plan(&response.plan, admission)
        .map_err(internal_contract)?;
    let outputs = PlanOutputs {
        matrix: canonical_json_str(&response.plan.matrix).map_err(internal_contract)?,
        plan_id: response.plan.plan_id.clone(),
        run_key: response.plan.run_key.clone(),
        covered_tasks: crate::covered_tasks::CoveredTasks::for_plan(&response.plan).encode(),
        qualification_campaign: qualification
            .map_or_else(String::new, |value| value.campaign.clone()),
        qualification_phase: qualification
            .map_or_else(String::new, |value| value.phase.as_str().to_owned()),
        qualification_cache_enabled: qualification.is_some_and(|value| value.phase.cache_enabled()),
        qualification_cache_write: false,
        qualification_cache_directives: directive
            .as_ref()
            .map(canonical_json_str)
            .transpose()
            .map_err(internal_contract)?
            .unwrap_or_default(),
        job_outputs_utf16_bytes: 0,
    };
    let bytes = check_plan_outputs(
        mode,
        response.plan.matrix.include.len(),
        &outputs.promoted_job_outputs(mode),
    )?;
    Ok(PlanOutputs {
        job_outputs_utf16_bytes: bytes,
        ..outputs
    })
}

/// Load the fixed runner-temp admission file before deriving qualification outputs.
/// # Errors
pub fn plan_outputs_from_staged_admission(
    response_json: &str,
    mode: PlanOutputMode,
    runner_temp: &std::path::Path,
) -> Result<PlanOutputs, OrchestratorError> {
    let admission =
        crate::qualification_resolver::read_qualification_admission(response_json, runner_temp)?;
    plan_outputs_with_admission(response_json, mode, admission.as_ref())
}

/// GitHub's maximum jobs created by one matrix strategy.
pub(crate) const MATRIX_JOB_LIMIT: usize = 256;
/// Conservative aggregate UTF-16 byte budget for one plan job's outputs.
///
/// GitHub documents a 1 MB per-job output limit, approximated in UTF-16.
/// Keeping 100,000 bytes in reserve avoids depending on service-side
/// accounting details.
pub const JOB_OUTPUTS_BUDGET_UTF16_BYTES: usize = 900_000;

/// Which outputs the plan job promotes to downstream jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOutputMode {
    /// Static crate jobs promote coverage and qualification policy outputs.
    Static,
    /// A task job promotes every plan-step output for matrix consumers.
    DynamicMatrix,
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
            (
                crate::QUALIFICATION_CAMPAIGN_OUTPUT,
                &self.qualification_campaign,
            ),
            (crate::QUALIFICATION_PHASE_OUTPUT, &self.qualification_phase),
            (
                crate::QUALIFICATION_CACHE_ENABLED_OUTPUT,
                bool_output(self.qualification_cache_enabled),
            ),
            (
                crate::QUALIFICATION_CACHE_WRITE_OUTPUT,
                bool_output(self.qualification_cache_write),
            ),
            (
                velnor_actions_contract::QUALIFICATION_CACHE_DIRECTIVES_OUTPUT,
                &self.qualification_cache_directives,
            ),
        ]
    }

    /// Output records actually promoted to job outputs by this workflow path.
    #[must_use]
    pub fn promoted_job_outputs(&self, mode: PlanOutputMode) -> Vec<(&'static str, &str)> {
        match mode {
            PlanOutputMode::Static => vec![
                (crate::COVERED_TASKS_OUTPUT, &self.covered_tasks),
                (
                    crate::QUALIFICATION_CAMPAIGN_OUTPUT,
                    &self.qualification_campaign,
                ),
                (crate::QUALIFICATION_PHASE_OUTPUT, &self.qualification_phase),
                (
                    crate::QUALIFICATION_CACHE_ENABLED_OUTPUT,
                    bool_output(self.qualification_cache_enabled),
                ),
                (
                    crate::QUALIFICATION_CACHE_WRITE_OUTPUT,
                    bool_output(self.qualification_cache_write),
                ),
                (
                    velnor_actions_contract::QUALIFICATION_CACHE_DIRECTIVES_OUTPUT,
                    &self.qualification_cache_directives,
                ),
            ],
            PlanOutputMode::DynamicMatrix => self.step_outputs(),
        }
    }
}

fn bool_output(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Check expanded matrix cardinality and the aggregate promoted outputs.
///
/// # Errors
///
/// Returns an internal planning error when the workflow would exceed a
/// platform limit or the conservative output budget.
pub(crate) fn check_plan_outputs(
    mode: PlanOutputMode,
    matrix_entries: usize,
    outputs: &[(&str, &str)],
) -> Result<usize, OrchestratorError> {
    if mode == PlanOutputMode::DynamicMatrix && matrix_entries > MATRIX_JOB_LIMIT {
        return Err(internal(&format!(
            "matrix_jobs_exceeded:{matrix_entries}:reduce matrix entries"
        )));
    }
    let bytes = outputs.iter().try_fold(0_usize, |total, (name, value)| {
        total.checked_add(output_record_utf16_bytes(name, value)?)
    });
    let Some(bytes) = bytes else {
        return Err(internal("job_outputs_size_overflow"));
    };
    if bytes > JOB_OUTPUTS_BUDGET_UTF16_BYTES {
        return Err(internal(&format!(
            "job_outputs_budget_exceeded:{bytes}:reduce matrix entries or covered work"
        )));
    }
    Ok(bytes)
}

/// UTF-16 bytes for one `$GITHUB_OUTPUT` record, including syntax.
fn output_record_utf16_bytes(name: &str, value: &str) -> Option<usize> {
    name.encode_utf16()
        .count()
        .checked_add(1)?
        .checked_add(value.encode_utf16().count())?
        .checked_add(1)?
        .checked_mul(2)
}

#[cfg(test)]
#[path = "plan_output_limits_tests.rs"]
mod tests;
