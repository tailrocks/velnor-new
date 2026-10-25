use crate::matrix::NOT_AFFECTED_JOBS_OUTPUT;
use crate::render::PLAN_JOB_ID;

#[must_use]
pub fn not_affected_gate(job_id: &str) -> String {
    format!("!contains(needs.{PLAN_JOB_ID}.outputs.{NOT_AFFECTED_JOBS_OUTPUT}, ',{job_id},')")
}
