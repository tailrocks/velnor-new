//! Workflow-IR and job construction for generation.
//!
//! Assembles the [`workflow`] plan (workflow IR plus renderer inputs)
//! and every IR job it carries: per-crate jobs ([`crate_jobs`], keyed
//! by [`crate_job_ids`]), the plan/lint/final jobs ([`workflow_jobs`]
//! plus [`workflow_jobs_cache`]), the baseline-publish job
//! ([`publish_job`]), verification-task jobs ([`verification_tasks`]),
//! and the shared catalog preflight steps ([`mbx_preflight`]).

pub mod crate_job_ids;
pub mod crate_jobs;
pub mod mbx_preflight;
pub mod publish_job;
pub mod verification_tasks;
pub mod workflow;
pub mod workflow_jobs;
pub mod workflow_jobs_cache;
