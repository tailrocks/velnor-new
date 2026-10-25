use crate::{
    OrchestratorError,
    internal::{PlanResponse, internal, internal_contract},
};
use velnor_actions_contract::{
    QualificationCacheAdmission, QualificationCacheDirective, canonical_json_str,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutputs {
    pub matrix: String,
    pub plan_id: String,
    pub run_key: String,
    pub not_affected_jobs: String,
    pub qualification_campaign: String,
    pub qualification_phase: String,
    pub qualification_cache_enabled: bool,
    pub qualification_cache_write: bool,
    pub qualification_cache_directives: String,
    pub job_outputs_utf16_bytes: usize,
}

pub fn plan_outputs(
    response_json: &str,
    mode: PlanOutputMode,
) -> Result<PlanOutputs, OrchestratorError> {
    plan_outputs_with_admission(response_json, mode, None)
}

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
        not_affected_jobs: crate::plan_dispositions::encode_not_affected_jobs(&response.plan.jobs),
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

pub fn plan_outputs_from_staged_admission(
    response_json: &str,
    mode: PlanOutputMode,
    runner_temp: &std::path::Path,
) -> Result<PlanOutputs, OrchestratorError> {
    let admission =
        crate::qualification_resolver::read_qualification_admission(response_json, runner_temp)?;
    plan_outputs_with_admission(response_json, mode, admission.as_ref())
}

pub(crate) const MATRIX_JOB_LIMIT: usize = 256;
pub const JOB_OUTPUTS_BUDGET_UTF16_BYTES: usize = 900_000;
pub(crate) const PLAN_STEP_OUTPUTS_BUDGET_UTF16_BYTES: usize = JOB_OUTPUTS_BUDGET_UTF16_BYTES
    - velnor_actions_contract::workflow::cache_family::PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOutputMode {
    Static,
    DynamicMatrix,
}

impl PlanOutputs {
    #[must_use]
    pub fn step_outputs(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("matrix", &self.matrix),
            ("plan_id", &self.plan_id),
            ("run_key", &self.run_key),
            (NOT_AFFECTED_JOBS_OUTPUT, &self.not_affected_jobs),
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

    #[must_use]
    pub fn promoted_job_outputs(&self, mode: PlanOutputMode) -> Vec<(&'static str, &str)> {
        match mode {
            PlanOutputMode::Static => vec![
                (NOT_AFFECTED_JOBS_OUTPUT, &self.not_affected_jobs),
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

use velnor_actions_workflow_renderer::render::NOT_AFFECTED_JOBS_OUTPUT;

fn bool_output(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

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
    if bytes > PLAN_STEP_OUTPUTS_BUDGET_UTF16_BYTES {
        return Err(internal(&format!(
            "job_outputs_budget_exceeded:{bytes}:reduce matrix entries"
        )));
    }
    Ok(bytes)
}

fn output_record_utf16_bytes(name: &str, value: &str) -> Option<usize> {
    name.encode_utf16()
        .count()
        .checked_add(1)?
        .checked_add(value.encode_utf16().count())?
        .checked_add(1)?
        .checked_mul(2)
}
