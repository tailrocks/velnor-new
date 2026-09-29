//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! The final job downloads the plan artifact (`plan.json`, `matrix.json`)
//! plus every matrix-report artifact under
//! `$RUNNER_TEMP/velnor/<run-key>/` before the merge step; this module
//! assembles those files into the canonical merge-request JSON that
//! [`crate::merge_internal`] consumes. Required job conclusions ride no
//! workflow channel yet, so the assembled request carries none; matrix
//! evidence alone drives the verdict until that channel lands.

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical_json_str;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

/// Assemble one canonical merge request from a run directory.
///
/// Reads `plan.json`, `matrix.json`, and every `*.json` file under
/// `reports/` (sorted by report ID for determinism; other filenames are
/// ignored). A missing `reports/` directory means zero reports, which the
/// merge judges honestly (`not_run` for pending entries, `no_work` for an
/// empty plan); a missing or unparsable plan or matrix fails closed.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing plan/matrix files
/// and [`OrchestratorError::Io`] for unreadable or unparsable JSON.
pub fn assemble_merge_request(run_key: &str, run_dir: &Path) -> Result<String, OrchestratorError> {
    let plan = read_json(run_dir, "plan.json")?;
    let matrix = read_json(run_dir, "matrix.json")?;
    let reports = read_reports(&run_dir.join("reports"))?;
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

/// Read one required JSON artifact from the run directory.
fn read_json(run_dir: &Path, name: &str) -> Result<serde_json::Value, OrchestratorError> {
    let path = run_dir.join(name);
    let text = fs::read_to_string(&path)
        .map_err(|_| internal(&format!("missing_plan_artifact:{name}")))?;
    serde_json::from_str(&text)
        .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))
}

/// Read every `*.json` matrix report, sorted by report ID.
///
/// A missing directory yields zero reports; unparsable files fail closed.
fn read_reports(dir: &Path) -> Result<Vec<serde_json::Value>, OrchestratorError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(OrchestratorError::io(
                dir.display().to_string(),
                err.to_string(),
            ));
        }
    };
    let mut reports = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|err| OrchestratorError::io(dir.display().to_string(), err.to_string()))?;
        let path = entry.path();
        let is_json = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if !is_json {
            continue;
        }
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

    /// Run directory with dummy plan/matrix plus caller-supplied files.
    fn staged(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("plan.json"), "{}").expect("plan");
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
        let dir = staged(&[("reports/a.json", r#"{"report_id":"b"}"#)]);
        let request = assemble_merge_request("local", dir.path()).expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert!(value.get("base").is_none(), "{request}");
        assert_eq!(value["schema"], 1);
        assert_eq!(value["run_key"], "local");
        assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn assembly_skips_non_json_and_sorts_reports() {
        let dir = staged(&[
            ("reports/b.json", r#"{"report_id":"report-2"}"#),
            ("reports/a.json", r#"{"report_id":"report-1"}"#),
            ("reports/note.txt", "ignored"),
        ]);
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
    fn assembly_rejects_missing_plan_and_bad_report() {
        let empty = tempfile::TempDir::new().expect("tempdir");
        let err = assemble_merge_request("local", empty.path()).expect_err("missing plan");
        assert!(err.to_string().contains("missing_plan_artifact"), "{err}");
        let bad = staged(&[("reports/a.json", "not json")]);
        let err = assemble_merge_request("local", bad.path()).expect_err("bad report");
        assert!(matches!(err, OrchestratorError::Io { .. }), "{err}");
    }

    #[test]
    fn request_file_writes_exclusively() {
        let dir = staged(&[]);
        let file = dir.path().join("sub").join("merge-v1-request.json");
        let written = write_merge_request_to(&file, "local", dir.path()).expect("write");
        assert_eq!(written, file);
        let err = write_merge_request_to(&file, "local", dir.path()).expect_err("exists");
        assert!(err.to_string().contains("request_exists"), "{err}");
    }
}
