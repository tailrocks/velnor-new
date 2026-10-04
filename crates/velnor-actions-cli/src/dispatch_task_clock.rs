//! File-less private dispatch for portable task start telemetry.

use std::env;
use std::process::ExitCode;

use velnor_actions_orchestrator::{START_OP, write_task_start};

/// Admit only the fixed operation under the ordinary report environment.
pub(crate) fn try_internal() -> Option<ExitCode> {
    if env::var("VELNOR_INTERNAL_OP").as_deref() != Ok(START_OP)
        || !env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty())
        || !env::var_os("RUNNER_TEMP").is_some_and(|path| !path.is_empty())
    {
        return None;
    }
    Some(match write_task_start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    })
}
