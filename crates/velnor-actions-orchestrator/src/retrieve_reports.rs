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

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{task_report_id_for_task, validate_artifact_id};
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

/// Maximum bytes read from one staged report file (P01-6 size bound).
///
/// Matrix and task reports are kilobytes; one megabyte fails closed on
/// runaway payloads without truncating legitimate evidence.
pub(crate) const MAX_STAGED_REPORT_BYTES: u64 = 1 << 20;

/// Read exactly the plan-expected staged reports plus task files.
///
/// Each `matrix.include` entry names its artifact; absent or unreadable
/// files are recorded in `errors`, never skipped silently. Stray files
/// are ignored. Both `gh` extract layouts are accepted (direct plus one
/// nested artifact directory); both paths are exact, never globbed. An
/// artifact ID that fails shape validation is recorded, never traversed.
/// Task files come from `<report-dir>/tasks/<task-report-id>.json` for
/// the plan-derived expectation only. Both lists sort by ID.
pub(crate) fn read_staged_reports(
    run_key: &str,
    plan: &serde_json::Value,
    dir: &Path,
    errors: &mut Vec<String>,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let digests = obligation_digests(plan);
    let entries = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array);
    let mut reports = Vec::new();
    let mut tasks = Vec::new();
    for entry in entries.into_iter().flatten() {
        let Some(artifact_id) = entry.get("artifact_id").and_then(serde_json::Value::as_str) else {
            errors.push("bad_artifact_id".to_owned());
            continue;
        };
        if validate_artifact_id(artifact_id).is_err() {
            errors.push(format!("bad_artifact_id:{artifact_id}"));
            continue;
        }
        let Some(home) = read_matrix_file(dir, artifact_id, &mut reports, errors) else {
            continue;
        };
        // Task files beside the matrix file for the plan-derived
        // expectation only; entries without that shape expect nothing
        // and the merge fails them closed on incoherence instead.
        let tasks_dir = home.join("tasks");
        let linked = is_symlink(&tasks_dir);
        for report_id in expected_file_ids(entry, run_key, &digests) {
            if linked {
                errors.push(format!("symlink_task:{report_id}"));
                continue;
            }
            match read_bounded(&tasks_dir.join(format!("{report_id}.json"))) {
                Ok(text) => match serde_json::from_str(&text) {
                    Ok(task) => tasks.push(task),
                    Err(_) => errors.push(format!("unparsable_task:{report_id}")),
                },
                Err("missing") => errors.push(format!("missing_task:{report_id}")),
                Err(kind) => errors.push(format!("{kind}_task:{report_id}")),
            }
        }
    }
    reports.sort_by(|left, right| staged_id(left, "report_id").cmp(staged_id(right, "report_id")));
    tasks.sort_by(|left, right| {
        staged_id(left, "task_report_id").cmp(staged_id(right, "task_report_id"))
    });
    (reports, tasks)
}

/// Obligation task digests keyed by task ID from an untrusted plan.
fn obligation_digests(plan: &serde_json::Value) -> BTreeMap<&str, &str> {
    let mut digests = BTreeMap::new();
    let obligations = plan
        .get("obligations")
        .and_then(serde_json::Value::as_array);
    for obligation in obligations.into_iter().flatten() {
        let id = obligation
            .get("task_id")
            .and_then(serde_json::Value::as_str);
        let digest = obligation
            .get("task_digest")
            .and_then(serde_json::Value::as_str);
        if let (Some(id), Some(digest)) = (id, digest) {
            digests.insert(id, digest);
        }
    }
    digests
}

/// Read one artifact's matrix file; the home dir on success.
///
/// The artifact directory itself must be a real directory: a symlink
/// anywhere on a traversed path rejects, even at a live target.
fn read_matrix_file(
    dir: &Path,
    artifact_id: &str,
    reports: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let home = dir.join(artifact_id);
    if is_symlink(&home) {
        errors.push(format!("symlink_report:{artifact_id}"));
        return None;
    }
    let direct = home.join("matrix-report.json");
    let nested = home.join(artifact_id).join("matrix-report.json");
    let (path, text) = match read_bounded(&direct) {
        Ok(text) => (direct, text),
        Err("missing") => {
            if nested.parent().is_some_and(is_symlink) {
                errors.push(format!("symlink_report:{artifact_id}"));
                return None;
            }
            match read_bounded(&nested) {
                Ok(text) => (nested, text),
                Err(kind) => {
                    errors.push(format!("{kind}_report:{artifact_id}"));
                    return None;
                }
            }
        }
        Err(kind) => {
            errors.push(format!("{kind}_report:{artifact_id}"));
            return None;
        }
    };
    let Ok(report) = serde_json::from_str(&text) else {
        errors.push(format!("unparsable_report:{artifact_id}"));
        return None;
    };
    reports.push(report);
    path.parent().map(Path::to_path_buf)
}

/// Plan-derived task-file IDs for one entry, sorted.
fn expected_file_ids(
    entry: &serde_json::Value,
    run_key: &str,
    digests: &BTreeMap<&str, &str>,
) -> Vec<String> {
    let Some(matrix_key) = entry.get("matrix_key").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    let refs = entry
        .get("execute_task_ids")
        .and_then(|ids| ids.get("tasks").or(Some(ids)))
        .and_then(serde_json::Value::as_object);
    let mut ids = Vec::new();
    for task_ref in refs.into_iter().flat_map(|tasks| tasks.values()) {
        let mut push = |id: &str| {
            let derived = digests
                .get(id)
                .and_then(|digest| task_report_id_for_task(run_key, matrix_key, digest).ok());
            ids.extend(derived);
        };
        if let Some(id) = task_ref.as_str() {
            push(id);
        } else if let Some(shards) = task_ref.as_array() {
            for shard in shards.iter().filter_map(serde_json::Value::as_str) {
                push(shard);
            }
        }
    }
    ids.sort();
    ids
}

/// True when a traversed path is a symlink.
///
/// `symlink_metadata` never follows the final component: a symlink
/// rejects even at a live target. Missing paths are not links; the
/// bounded read below reports them as missing instead.
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Read one staged file with symlink rejection and a size bound.
///
/// Symlinks reject without reading; reads stop one byte past the bound
/// so oversize files error instead of exhausting memory. Failure
/// classes map directly onto assembly error prefixes.
fn read_bounded(path: &Path) -> Result<String, &'static str> {
    read_staged_text(path, MAX_STAGED_REPORT_BYTES)
}

/// Read one file with symlink rejection and a caller size bound.
///
/// Shared by staged-report reads and merge-request assembly so every
/// event-time read enforces the same gates: symlinks and non-files
/// reject, missing files report, and oversize files error instead of
/// exhausting memory.
pub(crate) fn read_staged_text(path: &Path, bound: u64) -> Result<String, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err("symlink"),
        Ok(meta) if !meta.is_file() => return Err("unreadable"),
        Err(_) => return Err("missing"),
        Ok(_) => {}
    }
    let file = fs::File::open(path).map_err(|_| "unreadable")?;
    let mut text = String::new();
    file.take(bound + 1)
        .read_to_string(&mut text)
        .map_err(|_| "unreadable")?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > bound {
        return Err("oversize");
    }
    Ok(text)
}

/// Sort key for one staged value; empty when the ID is absent.
fn staged_id<'a>(report: &'a serde_json::Value, field: &str) -> &'a str {
    report
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
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
#[path = "retrieve_reports_tests.rs"]
mod retrieve_reports_tests;
