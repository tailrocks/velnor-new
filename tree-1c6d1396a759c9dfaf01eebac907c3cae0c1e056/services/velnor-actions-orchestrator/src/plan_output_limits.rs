//! Fail-closed limits for plan artifacts and job outputs.

use velnor_actions_orchestrator_core::{OrchestratorError, internal};

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
    /// Static crate jobs consume only the coverage output.
    Static,
    /// A task job expands the plan matrix and also consumes plan identity.
    DynamicMatrix,
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
mod tests;
