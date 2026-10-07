//! Event-time `write-task-report-v1` operation core.
//!
//! Extracted from the orchestrator hub: [`report_write`] binds task and
//! matrix identities through the staged plan with no hub dependency —
//! the run key arrives as an input and the plan bound is a private
//! value-matched constant — so this crate is a dependency-free leaf.
//! [`task_report`] keeps the environment-driven `write_task_report`
//! entrypoint.

mod report_write;
mod task_report;
mod timing;

pub use report_write::{write_task_report_to, write_task_report_with_key};
pub use task_report::write_task_report;
