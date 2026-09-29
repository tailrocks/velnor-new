//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! The final job downloads the plan artifact (`plan.json`, `matrix.json`)
//! plus every matrix-report artifact under `reports/<artifact-id>/` before
//! the merge step; this module assembles those files into the canonical
//! merge-request JSON that [`crate::merge_internal`] consumes. Required
//! job conclusions ride no workflow channel yet, so the assembled request
//! carries none; matrix evidence alone drives the verdict until that
//! channel lands.

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{canonical_json_str, validate_artifact_id};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

/// Assemble one canonical merge request from a run directory.
///
/// Reads `plan.json`, `matrix.json`, and exactly the plan-expected
/// `reports/<artifact-id>/matrix-report.json` files (sorted by report ID
/// for determinism; anything else under `reports/` is ignored, never
/// globbed). A missing `reports/` entry means zero reports for that leg,
/// which the merge judges honestly (`not_run` for pending entries,
/// `no_work` for an empty plan). A missing or unparsable plan or matrix
/// becomes JSON null so the merge still reaches its `planning_failed`
/// verdict instead of dying in request assembly.
///
/// # Errors
///
/// Returns [`OrchestratorError::Io`] for unreadable directories and
/// [`OrchestratorError::Internal`] for unparsable expected reports.
pub fn assemble_merge_request(run_key: &str, run_dir: &Path) -> Result<String, OrchestratorError> {
    let plan = read_optional_json(run_dir, "plan.json");
    let matrix = read_optional_json(run_dir, "matrix.json");
    let reports = read_expected_reports(&plan, &run_dir.join("reports"))?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": run_key,
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
    });
    canonical_json_str(&request).map_err(internal_contract)
}

/// Materialize the merge request from the environment and run directory.
///
/// Resolves the run key from `GITHUB_RUN_ID`/`GITHUB_RUN_ATTEMPT` and the
/// run directory from `RUNNER_TEMP`, then delegates to
/// [`write_merge_request_to`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env or unwritable
/// paths; [`OrchestratorError::Io`] for unreadable artifact JSON.
pub(crate) fn write_merge_request(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&temp).join("velnor").join(&run_key);
    write_merge_request_to(request_path, &run_key, &run_dir)
}

/// Assemble and exclusively write one merge request file.
///
/// The file is written exclusively (a pre-existing file errors, never
/// overwritten), matching the plan request writer.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing plan/matrix files
/// or a pre-existing request file; [`OrchestratorError::Io`] for
/// unreadable artifact JSON.
pub(crate) fn write_merge_request_to(
    request_path: &Path,
    run_key: &str,
    run_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let request = assemble_merge_request(run_key, run_dir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| internal("request_exists"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(request.as_bytes())
                .map_err(|_| internal("request_unwritable"))
        })?;
    Ok(path)
}

/// Read one optional JSON artifact; missing or unparsable becomes null.
fn read_optional_json(run_dir: &Path, name: &str) -> serde_json::Value {
    fs::read_to_string(run_dir.join(name))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Read exactly the plan-expected matrix reports, sorted by report ID.
///
/// Each `matrix.include` entry names its artifact; absent files mean the
/// leg never reported (merge judges `not_run`). Stray files are ignored.
/// Both `gh` extract layouts are accepted (direct plus one nested
/// artifact directory); both paths are exact, never globbed. A
/// present-but-unparsable expected report fails closed, as does an
/// artifact ID that fails shape validation (never a path traversal).
///
/// # Errors
///
/// Returns [`OrchestratorError::Io`] for unreadable or unparsable
/// expected reports and [`OrchestratorError::Internal`] for malformed
/// artifact IDs.
fn read_expected_reports(
    plan: &serde_json::Value,
    dir: &Path,
) -> Result<Vec<serde_json::Value>, OrchestratorError> {
    let mut expected = Vec::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            let artifact_id = entry.get("artifact_id").and_then(serde_json::Value::as_str);
            if let Some(artifact_id) = artifact_id {
                validate_artifact_id(artifact_id).map_err(internal_contract)?;
                expected.push(artifact_id);
            }
        }
    }
    let mut reports = Vec::new();
    for artifact_id in expected {
        let direct = dir.join(artifact_id).join("matrix-report.json");
        let nested = dir
            .join(artifact_id)
            .join(artifact_id)
            .join("matrix-report.json");
        let path = if direct.is_file() {
            direct
        } else if nested.is_file() {
            nested
        } else if direct.exists() || nested.exists() {
            return Err(OrchestratorError::io(
                direct.display().to_string(),
                "unreadable_matrix_report".to_owned(),
            ));
        } else {
            continue;
        };
        let text = fs::read_to_string(&path)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        let report: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        reports.push(report);
    }
    reports.sort_by(|left, right| report_id(left).cmp(report_id(right)));
    Ok(reports)
}

/// Sort key for one report value; empty when the ID is absent.
fn report_id(report: &serde_json::Value) -> &str {
    report
        .get("report_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal plan JSON naming the given artifact IDs.
    fn plan_with(artifact_ids: &[&str]) -> String {
        let include: Vec<String> = artifact_ids
            .iter()
            .map(|id| format!(r#"{{"artifact_id":"{id}","report_id":"report-for-{id}"}}"#))
            .collect();
        format!(r#"{{"matrix":{{"include":[{}]}}}}"#, include.join(","))
    }

    /// Run directory with caller-supplied plan plus caller-supplied files.
    fn staged(plan: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("plan.json"), plan).expect("plan");
        std::fs::write(dir.path().join("matrix.json"), "{}").expect("matrix");
        for (name, body) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parents");
            }
            std::fs::write(&path, body).expect("file");
        }
        dir
    }

    #[test]
    fn assembly_shape_carries_no_base() {
        let aid = "velnor-matrix-local-m-0123456789abcdef";
        let dir = staged(
            &plan_with(&[aid]),
            &[(
                "reports/velnor-matrix-local-m-0123456789abcdef/matrix-report.json",
                r#"{"report_id":"b"}"#,
            )],
        );
        let request = assemble_merge_request("local", dir.path()).expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert!(value.get("base").is_none(), "{request}");
        assert_eq!(value["schema"], 1);
        assert_eq!(value["run_key"], "local");
        assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn assembly_reads_expected_only_and_sorts_reports() {
        let first = "velnor-matrix-local-m-0000000000000001";
        let second = "velnor-matrix-local-m-0000000000000002";
        let dir = staged(
            &plan_with(&[first, second]),
            &[
                (
                    "reports/velnor-matrix-local-m-0000000000000002/matrix-report.json",
                    r#"{"report_id":"report-2"}"#,
                ),
                (
                    "reports/velnor-matrix-local-m-0000000000000001/matrix-report.json",
                    r#"{"report_id":"report-1"}"#,
                ),
                ("reports/stray.json", r#"{"report_id":"stray"}"#),
                (
                    "reports/velnor-matrix-local-m-9999999999999999/matrix-report.json",
                    r#"{"report_id":"unexpected"}"#,
                ),
            ],
        );
        let request = assemble_merge_request("local", dir.path()).expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        let ids: Vec<&str> = value["matrix_reports"]
            .as_array()
            .expect("reports")
            .iter()
            .map(|report| report["report_id"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(ids, ["report-1", "report-2"]);
    }

    #[test]
    fn assembly_nulls_missing_plan_and_rejects_bad_report() {
        let empty = tempfile::TempDir::new().expect("tempdir");
        let request = assemble_merge_request("local", empty.path()).expect("null plan");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert!(value["plan"].is_null(), "{request}");
        assert!(value["matrix"].is_null(), "{request}");
        assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(0));
        let aid = "velnor-matrix-local-m-0123456789abcdef";
        let bad = staged(
            &plan_with(&[aid]),
            &[(
                "reports/velnor-matrix-local-m-0123456789abcdef/matrix-report.json",
                "not json",
            )],
        );
        let err = assemble_merge_request("local", bad.path()).expect_err("bad report");
        assert!(matches!(err, OrchestratorError::Io { .. }), "{err}");
    }

    #[test]
    fn request_file_writes_exclusively() {
        let dir = staged("{}", &[]);
        let file = dir.path().join("sub").join("merge-v1-request.json");
        let written = write_merge_request_to(&file, "local", dir.path()).expect("write");
        assert_eq!(written, file);
        let err = write_merge_request_to(&file, "local", dir.path()).expect_err("exists");
        assert!(err.to_string().contains("request_exists"), "{err}");
    }
}
