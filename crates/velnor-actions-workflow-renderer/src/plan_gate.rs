//! The job-level gate that skips jobs the plan marks `not_affected`.

use crate::matrix::NOT_AFFECTED_JOBS_OUTPUT;
use crate::render::PLAN_JOB_ID;

/// Job-level gate skipping one job the plan marks `not_affected`.
///
/// Negative list over [`NOT_AFFECTED_JOBS_OUTPUT`] (`,a,b,`): an empty or
/// missing output matches nothing and runs the job, and the implicit
/// `success()` skips it when the plan fails (hard decision 3).
#[must_use]
pub fn not_affected_gate(job_id: &str) -> String {
    format!("!contains(needs.{PLAN_JOB_ID}.outputs.{NOT_AFFECTED_JOBS_OUTPUT}, ',{job_id},')")
}
