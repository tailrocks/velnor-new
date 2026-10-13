//! Hosted qualification admission dispatch and plan-output input handling.

use std::env;
use std::path::Path;

use velnor_actions_contract::QualificationCacheAdmission;
use velnor_actions_orchestrator::{
    load_qualification_admission, resolve_qualification_admission,
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
    response: &str,
    request_path: &Path,
) -> Result<Option<QualificationCacheAdmission>, String> {
    if qualification_lineage_required(response)? {
        load_qualification_admission(request_path).map_err(|error| error.to_string())
    } else {
        Ok(None)
    }
}

fn qualification_lineage_required(response: &str) -> Result<bool, String> {
    let value: serde_json::Value =
        serde_json::from_str(response).map_err(|_| "malformed plan response".to_owned())?;
    let plan = value
        .get("plan")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "malformed plan response".to_owned())?;
    let Some(qualification) = plan.get("qualification") else {
        return Ok(false);
    };
    let Some(qualification) = qualification.as_object() else {
        return if qualification.is_null() {
            Ok(false)
        } else {
            Err("malformed qualification context".to_owned())
        };
    };
    match qualification.get("phase").and_then(serde_json::Value::as_str) {
        Some("cold" | "control") => Ok(false),
        Some("warm" | "third" | "useful_delta") => Ok(true),
        _ => Err("invalid qualification phase".to_owned()),
    }
}

#[cfg(test)]
#[path = "dispatch_qualification_tests.rs"]
mod tests;
