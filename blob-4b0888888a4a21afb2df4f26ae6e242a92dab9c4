//! Private dispatch boundary for hosted qualification predecessor resolution.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator::{QUALIFICATION_RESOLVER_OP, resolve_qualification_admission};

use crate::dispatch;

pub(crate) fn is_resolver_op(operation: &str) -> bool {
    operation == QUALIFICATION_RESOLVER_OP
}

pub(crate) fn request_is_eligible(path: &Path) -> bool {
    path.is_file()
        && env::var("GITHUB_EVENT_NAME").as_deref() == Ok("workflow_dispatch")
        && env::var("GH_TOKEN").is_ok_and(|token| !token.is_empty())
}

pub(crate) fn run(path: &Path) -> ExitCode {
    match resolve_qualification_admission(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => dispatch::fail_internal(&error.to_string()),
    }
}
