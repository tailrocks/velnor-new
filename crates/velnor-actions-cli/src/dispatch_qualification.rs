//! Hosted qualification admission dispatch and plan-output input handling.

use std::env;
use std::path::Path;

use velnor_actions_orchestrator::{
    QualificationCacheAdmission, load_qualification_admission, resolve_qualification_admission,
};

/// Check the private resolver gate without exposing its operation tag.
pub(crate) fn gate(path: &Path) -> bool {
    path.is_file()
        && env::var_os("RUNNER_TEMP").is_some_and(|value| !value.is_empty())
        && env::var("GH_REPO").is_ok_and(|value| !value.trim().is_empty())
        && env::var("GH_TOKEN").is_ok_and(|value| !value.trim().is_empty())
}

/// Resolve and stage the immutable predecessor receipt.
pub(crate) fn run(path: &Path) -> Result<(), String> {
    resolve_qualification_admission(path).map_err(|error| error.to_string())
}

/// Load staged evidence only for phases that require an admitted predecessor.
pub(crate) fn admission_for_plan(
    request_path: &Path,
) -> Result<Option<QualificationCacheAdmission>, String> {
    load_qualification_admission(request_path).map_err(|error| error.to_string())
}
