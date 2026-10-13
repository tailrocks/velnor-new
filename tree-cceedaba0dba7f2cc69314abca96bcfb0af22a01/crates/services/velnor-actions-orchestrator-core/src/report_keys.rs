//! Wire keys shared by step-env producers and the report wrapper.
//!
//! Plan-time step construction (provisioning) and run-time report
//! production (the `velnor-actions` binary) agree on these keys by
//! construction: a single definition here, never duplicated per side.

/// Report-production operation tag.
pub const REPORT_OP: &str = "write-task-report-v1";
/// Env key carrying the executed obligation's task ID.
pub const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the captured obligation exit code.
pub const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
/// Env key carrying comma-separated downstream task IDs for skip reports.
pub const DOWNSTREAM_IDS_ENV: &str = "VELNOR_DOWNSTREAM_TASK_IDS";
/// Env key carrying the wrapper-captured start time (unix millis).
pub const START_MS_ENV: &str = "VELNOR_START_MS";
