//! Public dispatch for the repository freshness gate.

use std::path::Path;
use std::process::ExitCode;

/// Run native repository freshness validation.
pub(crate) fn run(root: &Path, check_upstream: bool, with_advisories: bool) -> ExitCode {
    match velnor_actions_orchestrator::check_freshness(root, check_upstream, with_advisories) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("check-freshness: output failed ({error})");
            ExitCode::FAILURE
        }
    }
}
