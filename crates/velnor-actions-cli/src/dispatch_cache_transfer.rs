//! Run typed cache payload operations.

use std::process::ExitCode;

pub(crate) const EXPORT_OP: &str = "export-qualification-cache-payload-v1";
pub(crate) const IMPORT_OP: &str = "import-qualification-cache-payload-v1";

#[path = "dispatch_cache_transfer_normal.rs"]
pub(crate) mod normal;
#[path = "dispatch_cache_transfer_operation.rs"]
mod qualification;

pub(crate) fn run_export() -> ExitCode {
    qualification::run_export()
}

pub(crate) fn run_import() -> ExitCode {
    qualification::run_import()
}

pub(crate) fn validate_root_observation(
    request: &velnor_actions_orchestrator::QualificationCacheBindingRequest<'_>,
    roots_json: &str,
) -> Result<(), &'static str> {
    qualification::validate_root_observation(request, roots_json)
}
