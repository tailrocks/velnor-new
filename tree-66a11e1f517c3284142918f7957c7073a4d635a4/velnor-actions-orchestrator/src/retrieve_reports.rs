//! Event-time `fetch-reports-v1`: exact matrix-artifact retrieval.
//!
//! The final job's retrieve step runs before request assembly: it reads
//! the downloaded plan, then downloads each expected matrix artifact by exact
//! derived name with pinned `gh` (`gh run download <run-id> --name
//! <artifact-id> --dir reports/<artifact-id>`), never a wildcard. One
//! failed download skips that leg (merge judges `not_run`); a missing
//! or unparsable plan downloads nothing and still exits success so the
//! merge reaches its `planning_failed` verdict. Only unusable
//! environment (no runner temp, no numeric run ID) fails outright.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use velnor_actions_contract::validate_artifact_id;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Retrieve operation tag (single-sourced from the renderer protocol).
pub use velnor_actions_workflow_renderer::steps::FETCH_OPERATION as FETCH_OP;

/// Retrieve every plan-expected matrix artifact for this run.
///
/// Resolves the run ID from `GITHUB_RUN_ID` and the run directory from
/// `RUNNER_TEMP/velnor/<run-key>`, then delegates to
/// `retrieve_reports_to`. Returns the count of artifacts downloaded.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing or non-numeric
/// run IDs and missing runner temp.
pub fn retrieve_reports() -> Result<usize, OrchestratorError> {
    let run_id = std::env::var("GITHUB_RUN_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_run_id"))?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_id"))?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&temp).join("velnor").join(&run_key);
    Ok(retrieve_reports_to(run_id, &run_dir))
}

/// Download each plan-expected artifact into `reports/<artifact-id>/`.
///
/// A missing or unparsable plan means zero downloads (the merge still
/// runs and reports `planning_failed`). Malformed artifact IDs are
/// skipped without spawning; failed downloads are skipped per leg.
pub(crate) fn retrieve_reports_to(run_id: u64, run_dir: &Path) -> usize {
    let plan = read_plan(run_dir);
    let Some(plan) = plan else {
        return 0;
    };
    let catalog = ToolCatalog::pinned();
    let mut retrieved = 0usize;
    for artifact_id in expected_artifact_ids(&plan) {
        let dir = run_dir.join("reports").join(artifact_id);
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let Ok(args) = retrieve_args(run_id, artifact_id, &dir) else {
            continue;
        };
        if crate::cover::shard::BaselineLookup::run(&catalog, run_dir, args).is_ok() {
            retrieved += 1;
        }
    }
    retrieved
}

/// Fixed `gh run download` argv for one exact artifact (no wildcards).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a malformed artifact ID.
pub(crate) fn retrieve_args(
    run_id: u64,
    artifact_id: &str,
    dir: &Path,
) -> Result<Vec<OsString>, OrchestratorError> {
    validate_artifact_id(artifact_id).map_err(internal_contract)?;
    Ok(vec![
        OsString::from("run"),
        OsString::from("download"),
        OsString::from(run_id.to_string()),
        OsString::from("--name"),
        OsString::from(artifact_id),
        OsString::from("--dir"),
        dir.as_os_str().to_owned(),
    ])
}

/// Parse the downloaded plan, if any.
fn read_plan(run_dir: &Path) -> Option<serde_json::Value> {
    let text = fs::read_to_string(run_dir.join("plan.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Expected matrix artifact IDs from a plan value, in plan order.
fn expected_artifact_ids(plan: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            if let Some(id) = entry.get("artifact_id").and_then(serde_json::Value::as_str) {
                ids.push(id);
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieve_argv_names_exact_artifact() {
        let args = retrieve_args(
            7,
            "velnor-matrix-r7-a2-m-0123456789abcdef",
            Path::new("/tmp/x"),
        )
        .expect("argv");
        let text: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            text,
            [
                "run",
                "download",
                "7",
                "--name",
                "velnor-matrix-r7-a2-m-0123456789abcdef",
                "--dir",
                "/tmp/x"
            ]
        );
        assert!(retrieve_args(7, "velnor-matrix-*", Path::new("/tmp/x")).is_err());
        assert!(retrieve_args(7, "../escape", Path::new("/tmp/x")).is_err());
    }

    #[test]
    fn missing_plan_retrieves_zero_without_spawning() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let run = dir.path().join("r7-a2");
        assert_eq!(retrieve_reports_to(7, &run), 0);
        assert!(!run.join("reports").exists(), "no downloads attempted");
    }

    #[test]
    fn enumeration_follows_plan_order() {
        let plan = serde_json::json!({
            "matrix": {"include": [
                {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000002"},
                {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000001"},
            ]}
        });
        assert_eq!(
            expected_artifact_ids(&plan),
            [
                "velnor-matrix-r7-a2-m-0000000000000002",
                "velnor-matrix-r7-a2-m-0000000000000001",
            ]
        );
        assert!(expected_artifact_ids(&serde_json::json!({})).is_empty());
    }
}
