//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! The final job downloads the plan artifact (`plan.json`, `matrix.json`)
//! plus every matrix-report artifact under `reports/<artifact-id>/` before
//! the merge step; this module assembles those files into the canonical
//! merge-request JSON that [`crate::merge_internal`] consumes.
//!
//! Required validator conclusions arrive through the `VELNOR_NEEDS_JSON`
//! channel: a JSON object mapping each job in the final gate's `needs`
//! to its conclusion, either directly (`{"plan": "success"}`) or in
//! `toJSON(needs)` shape (`{"plan": {"result": "success"}}`). The
//! renderer emits the finalized `needs` set; assembly declares it as the
//! required inventory (minus the matrix-driver job, whose legs prove
//! themselves through per-leg reports) and fails closed on a missing or
//! unparsable channel.
//!
//! Assembly never drops evidence silently: every missing or unparsable
//! artifact becomes an explicit `assembly_errors` entry that fails the
//! verdict, so the merge still emits its diagnostic `planning_failed`
//! report instead of dying in request assembly.

// Needs-channel parsing lives beside assembly so `lib.rs` stays untouched.
#[path = "needs_channel.rs"]
mod needs_channel;

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical_json_str;

use self::needs_channel::{NEEDS_ENV, parse_needs};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

/// Assemble one canonical merge request from a run directory.
///
/// Reads `plan.json`, `matrix.json`, exactly the plan-expected
/// `reports/<artifact-id>/matrix-report.json` files plus their
/// `tasks/<task-report-id>.json` files (sorted by ID for determinism;
/// anything else under `reports/` is ignored, never globbed), plus
/// optional `baseline.json` and `candidate-report.json`.
/// Validator inventory and conclusions come from [`NEEDS_ENV`]. Every
/// missing or unparsable input is recorded in `assembly_errors`, never
/// dropped, so the merge judges the gap explicitly.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
pub fn assemble_merge_request(run_key: &str, run_dir: &Path) -> Result<String, OrchestratorError> {
    let needs = std::env::var(NEEDS_ENV).ok();
    assemble_with_needs(run_key, run_dir, needs.as_deref())
}

/// Assemble one merge request with an explicit needs channel.
///
/// The public wrapper reads [`NEEDS_ENV`]; tests pass the channel
/// explicitly for determinism.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
fn assemble_with_needs(
    run_key: &str,
    run_dir: &Path,
    needs: Option<&str>,
) -> Result<String, OrchestratorError> {
    let mut errors = Vec::new();
    let plan = read_json(run_dir, "plan.json", "plan", true, &mut errors);
    let matrix = read_json(run_dir, "matrix.json", "matrix", true, &mut errors);
    let (reports, task_reports) = crate::retrieve_reports::read_staged_reports(
        run_key,
        &plan,
        &run_dir.join("reports"),
        &mut errors,
    );
    let baseline = read_json(run_dir, "baseline.json", "baseline", false, &mut errors);
    let candidate = read_json(
        run_dir,
        "candidate-report.json",
        "candidate_report",
        false,
        &mut errors,
    );
    let (inventory, jobs) = parse_needs(needs, &mut errors);
    let request = serde_json::json!({
        "schema": 1,
        "run_key": run_key,
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "task_reports": task_reports,
        "required_job_ids": inventory,
        "required_jobs": jobs,
        "assembly_errors": errors,
        "baseline_manifest": baseline,
        "candidate": candidate,
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
/// overwritten), matching the plan request writer. Missing inputs are
/// recorded in the request, not refused here.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a pre-existing request
/// file or unwritable paths.
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

/// Read one JSON artifact; failures become null plus an explicit error.
///
/// Absence errors only for required artifacts; corruption always errors.
fn read_json(
    run_dir: &Path,
    name: &str,
    kind: &str,
    required: bool,
    errors: &mut Vec<String>,
) -> serde_json::Value {
    let Ok(text) = fs::read_to_string(run_dir.join(name)) else {
        if required {
            errors.push(format!("missing_{kind}"));
        }
        return serde_json::Value::Null;
    };
    let Ok(value) = serde_json::from_str(&text) else {
        errors.push(format!("unparsable_{kind}"));
        return serde_json::Value::Null;
    };
    value
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

    /// Error strings of one assembled request.
    fn error_list(request: &str) -> Vec<String> {
        serde_json::from_str::<serde_json::Value>(request).expect("json")["assembly_errors"]
            .as_array()
            .expect("errors")
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
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
        let needs = r#"{"plan":"success","rust-demo":"success"}"#;
        let request = assemble_with_needs("local", dir.path(), Some(needs)).expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert!(value.get("base").is_none(), "{request}");
        assert_eq!(value["schema"], 1);
        assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            value["required_job_ids"],
            serde_json::json!(["plan", "rust-demo"])
        );
        assert!(error_list(&request).is_empty(), "{request}");
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
            ],
        );
        let needs = r#"{"plan":{"result":"success","outputs":{}}}"#;
        let request = assemble_with_needs("local", dir.path(), Some(needs)).expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        let ids: Vec<&str> = value["matrix_reports"]
            .as_array()
            .expect("reports")
            .iter()
            .map(|report| report["report_id"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(ids, ["report-1", "report-2"]);
        assert!(error_list(&request).is_empty(), "{request}");
    }

    #[test]
    fn assembly_records_gaps_and_rejects_bad_report() {
        let empty = tempfile::TempDir::new().expect("tempdir");
        let request = assemble_with_needs("local", empty.path(), None).expect("null plan");
        let value: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert!(value["plan"].is_null(), "{request}");
        assert!(value["matrix"].is_null(), "{request}");
        assert_eq!(value["required_job_ids"].as_array().map(Vec::len), Some(0));
        let errors = error_list(&request);
        for want in ["missing_plan", "missing_matrix", "missing_needs_channel"] {
            assert!(errors.contains(&want.to_owned()), "{errors:?}");
        }
        let aid = "velnor-matrix-local-m-0123456789abcdef";
        let bad = staged(
            &plan_with(&[aid]),
            &[(
                "reports/velnor-matrix-local-m-0123456789abcdef/matrix-report.json",
                "not json",
            )],
        );
        let request =
            assemble_with_needs("local", bad.path(), Some(r#"{"a":"b"}"#)).expect("diagnostic");
        let errors = error_list(&request);
        assert!(
            errors
                .iter()
                .any(|error| error.starts_with("unparsable_report:")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.starts_with("bad_needs_result:")),
            "{errors:?}"
        );
        let missing = staged(&plan_with(&[aid]), &[]);
        let request = assemble_with_needs("local", missing.path(), Some(r#"{"plan":"success"}"#))
            .expect("diagnostic");
        let errors = error_list(&request);
        assert!(
            errors
                .iter()
                .any(|error| error.starts_with("missing_report:")),
            "{errors:?}"
        );
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
