//! Staged-plan task-report primitives for event-time reporting.
//!
//! [`task_report`] loads the downloaded plan under a caller-supplied
//! byte bound, resolves obligation entries, derives terminal task
//! reports, and writes canonical task plus matrix report files;
//! [`task_report_aggregate`] builds the single-task aggregate. The
//! hub keeps the environment-driven `write-task-report-v1` operation
//! and orchestrates these primitives with no-op and downstream policy.

pub mod task_report;
pub mod task_report_aggregate;
