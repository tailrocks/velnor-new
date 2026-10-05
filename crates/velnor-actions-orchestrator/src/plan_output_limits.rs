//! Fail-closed limits for plan artifacts and job outputs.

use crate::{OrchestratorError, internal::internal};
use crate::internal_request::PlanOutputs;

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
            (crate::QUALIFICATION_CAMPAIGN_OUTPUT, &self.qualification_campaign),
            (crate::QUALIFICATION_PHASE_OUTPUT, &self.qualification_phase),
            (
                crate::QUALIFICATION_CACHE_ENABLED_OUTPUT,
                bool_output(self.qualification_cache_enabled),
            ),
            (
                crate::QUALIFICATION_CACHE_WRITE_OUTPUT,
                bool_output(self.qualification_cache_write),
            ),
        ]
    }

    /// Output records actually promoted to job outputs by this workflow path.
    #[must_use]
    pub fn promoted_job_outputs(&self, mode: PlanOutputMode) -> Vec<(&'static str, &str)> {
        match mode {
            PlanOutputMode::Static => vec![
                (crate::COVERED_TASKS_OUTPUT, &self.covered_tasks),
                (crate::QUALIFICATION_CAMPAIGN_OUTPUT, &self.qualification_campaign),
                (crate::QUALIFICATION_PHASE_OUTPUT, &self.qualification_phase),
                (
                    crate::QUALIFICATION_CACHE_ENABLED_OUTPUT,
                    bool_output(self.qualification_cache_enabled),
                ),
                (
                    crate::QUALIFICATION_CACHE_WRITE_OUTPUT,
                    bool_output(self.qualification_cache_write),
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
