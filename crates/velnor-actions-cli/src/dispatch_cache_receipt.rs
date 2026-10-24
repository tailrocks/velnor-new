//! Required-job producer receipt operation.

use std::env;
use std::process::ExitCode;

use super::{cache_bind, fail_internal};

pub(crate) fn run() -> ExitCode {
    let path = match velnor_actions_orchestrator::write_qualification_cache_receipt() {
        Ok(path) => path,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output) = env::var_os(cache_bind::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail_internal("missing_github_output");
    };
    let Some(path) = path
        .to_str()
        .filter(|path| !path.chars().any(|value| matches!(value, '\n' | '\r')))
    else {
        return fail_internal("qualification_receipt_path_encoding");
    };
    let body = format!("receipt_path={path}\n");
    match cache_bind::append_owned_output(&output, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append_github_output:{error}")),
    }
}
