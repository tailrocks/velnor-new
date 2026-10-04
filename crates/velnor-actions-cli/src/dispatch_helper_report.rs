//! File-less private dispatch for compiled native obligation evidence.

use std::env;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    HELPER_BEGIN_OP, HELPER_REPORT_OP, RUST_REPORT_PREEXEC_OP, begin_helper_obligation_report,
    validate_rust_report_preexec, write_helper_obligation_report,
};

/// Apply the same private report gate before public command parsing.
pub(crate) fn try_internal() -> Option<ExitCode> {
    let operation = env::var("VELNOR_INTERNAL_OP").ok()?;
    if operation == RUST_REPORT_PREEXEC_OP {
        return Some(match validate_rust_report_preexec() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => super::fail_internal(&error.to_string()),
        });
    }
    let handler = match operation.as_str() {
        HELPER_BEGIN_OP => begin_helper_obligation_report,
        HELPER_REPORT_OP => write_helper_obligation_report,
        _ => return None,
    };
    if env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty())
        && env::var_os("RUNNER_TEMP").is_some_and(|path| !path.is_empty())
    {
        return Some(match handler() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => super::fail_internal(&error.to_string()),
        });
    }
    None
}
