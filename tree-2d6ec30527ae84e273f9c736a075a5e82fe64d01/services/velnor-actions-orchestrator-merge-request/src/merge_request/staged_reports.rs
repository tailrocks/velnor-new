//! Staged matrix/task report reads for merge-request assembly.
//!
//! Moved with its sole production caller out of the orchestrator hub:
//! assembly reads exactly the plan-expected files under `reports/`,
//! so the reader lives beside assembly. The hub re-exports it for
//! its retrieve-path unit tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    parse_strict_json, task_report_id_for_task, validate_artifact_id, validate_matrix_key,
};
use velnor_actions_orchestrator_core::staged_reads::{path_is_symlink, read_staged_text};

/// Maximum bytes read from one staged report file (P01-6 size bound).
///
/// Matrix and task reports are kilobytes; one megabyte fails closed on
/// runaway payloads without truncating legitimate evidence.
pub const MAX_STAGED_REPORT_BYTES: u64 = 1 << 20;

/// Read exactly the plan-expected staged reports plus task files.
///
/// Each `matrix.include` entry names its job's artifact and its own
/// matrix key; absent or unreadable files are recorded in `errors`,
/// never skipped silently. Stray files are ignored. Both `gh` extract
/// layouts are accepted (direct plus one nested artifact directory);
/// both paths are exact, never globbed. An artifact ID or matrix key
/// that fails shape validation is recorded, never traversed. Task
/// files come from `<report-dir>/tasks/<task-report-id>.json` for the
/// plan-derived expectation only. Both lists sort by ID.
pub fn read_staged_reports(
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
        let Some(matrix_key) = entry.get("matrix_key").and_then(serde_json::Value::as_str) else {
            errors.push(format!("bad_matrix_key:{artifact_id}"));
            continue;
        };
        if validate_matrix_key(matrix_key).is_err() {
            errors.push(format!("bad_matrix_key:{artifact_id}"));
            continue;
        }
        let Some(home) = read_matrix_file(dir, artifact_id, matrix_key, &mut reports, errors)
        else {
            continue;
        };
        // Task files beside the matrix file for the plan-derived
        // expectation only; entries without that shape expect nothing
        // and the merge fails them closed on incoherence instead.
        let tasks_dir = home.join("tasks");
        let linked = path_is_symlink(&tasks_dir);
        for report_id in expected_file_ids(entry, run_key, &digests) {
            if linked {
                errors.push(format!("symlink_task:{report_id}"));
                continue;
            }
            match read_bounded(&tasks_dir.join(format!("{report_id}.json"))) {
                Ok(text) => match parse_strict_json(&text) {
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

/// Read one entry's matrix file from its job's artifact; home on success.
///
/// The job artifact carries the whole run directory, so the entry's
/// report lives at `<matrix-key>/matrix-report.json` inside it. The
/// artifact home, the nested parent, and the file itself reject
/// symlinks (pre-checks plus a `NOFOLLOW` open validated via the
/// handle), even at live targets. Ancestors above the staging root are
/// trusted: the runner and the retrieve step create them.
fn read_matrix_file(
    dir: &Path,
    artifact_id: &str,
    matrix_key: &str,
    reports: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let home = dir.join(artifact_id);
    if path_is_symlink(&home) {
        errors.push(format!("symlink_report:{artifact_id}"));
        return None;
    }
    let entry_dir = home.join(matrix_key);
    if path_is_symlink(&entry_dir) {
        errors.push(format!("symlink_report:{artifact_id}"));
        return None;
    }
    let direct = entry_dir.join("matrix-report.json");
    let nested_entry = home.join(artifact_id).join(matrix_key);
    let nested = nested_entry.join("matrix-report.json");
    let (path, text) = match read_bounded(&direct) {
        Ok(text) => (direct, text),
        Err("missing") => {
            if path_is_symlink(&home.join(artifact_id)) || path_is_symlink(&nested_entry) {
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
    let Ok(report) = parse_strict_json(&text) else {
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

/// Read one staged file with symlink rejection and a size bound.
///
/// Symlinks reject without reading; reads stop one byte past the bound
/// so oversize files error instead of exhausting memory. Failure
/// classes map directly onto assembly error prefixes.
fn read_bounded(path: &Path) -> Result<String, &'static str> {
    read_staged_text(path, MAX_STAGED_REPORT_BYTES)
}

/// Sort key for one staged value; empty when the ID is absent.
fn staged_id<'a>(report: &'a serde_json::Value, field: &str) -> &'a str {
    report
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}
