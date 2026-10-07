//! Event-time `fetch-reports-v1`: exact crate-artifact retrieval.
//!
//! The final job's retrieve step runs before request assembly: it reads
//! the downloaded plan, then downloads each expected crate artifact by exact
//! derived name with pinned `gh` (`gh run download <run-id> --name
//! <artifact-id> --dir reports/<artifact-id>`), never a wildcard. Each
//! leg retries transient failures up to
//! [`crate::retrieve_retry::MAX_DOWNLOAD_ATTEMPTS`];
//! persistent failure skips that job's entries (merge judges
//! `not_run`). A missing or unparsable plan downloads nothing and
//! still exits success so the merge reaches its `planning_failed`
//! verdict. Only unusable environment (no runner temp, no numeric
//! run ID) fails outright.

// Wired here so the shared reader compiles without touching `lib.rs`.
pub(crate) mod staged_reads;
pub(crate) use self::staged_reads::{path_is_symlink, read_staged_bytes, read_staged_text};

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    parse_strict_json, task_report_id_for_task, validate_artifact_id, validate_matrix_key,
};
use velnor_actions_mise::ToolCatalog;

use crate::retrieve_retry::download_with_retry;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::{internal, internal_contract};

/// Retrieve operation tag (single-sourced from the renderer protocol).
pub use velnor_actions_workflow_steps::steps::FETCH_OPERATION as FETCH_OP;

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
/// Afterwards the plan's exact baseline is best-effort fetched beside
/// them; the merge judges its absence as `source_missing` itself.
pub(crate) fn retrieve_reports_to(run_id: u64, run_dir: &Path) -> usize {
    let plan = read_plan(run_dir);
    let Some(plan) = plan else {
        return 0;
    };
    let repo = std::env::var(velnor_actions_orchestrator_core::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .and_then(|raw| velnor_actions_orchestrator_core::origin::validate_repository_slug(&raw));
    let Some(repo) = repo else {
        return 0;
    };
    let catalog = ToolCatalog::pinned();
    let mut retrieved = 0usize;
    for artifact_id in expected_artifact_ids(&plan) {
        // Validate BEFORE join/mkdir: a malformed ID must never become
        // a path or a directory (X7).
        if validate_artifact_id(artifact_id).is_err() {
            continue;
        }
        let dir = run_dir.join("reports").join(artifact_id);
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let Ok(args) = retrieve_args(run_id, artifact_id, &dir, &repo) else {
            continue;
        };
        let (downloaded, _) = download_with_retry(|| {
            crate::cover::shard::BaselineLookup::run(&catalog, run_dir, args.clone()).is_ok()
        });
        if downloaded {
            retrieved += 1;
        }
    }
    crate::retrieve_baseline::retrieve_baseline_to(&catalog, run_dir, &plan, &repo);
    retrieved
}

/// Fixed `gh run download` argv for one exact artifact (no wildcards).
///
/// `--repo` pins the download to the expected repository: without it
/// `gh` would resolve the repo from the working directory's git
/// origin, which a prior step may have rewritten.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a malformed artifact ID
/// or a malformed repository slug.
pub(crate) fn retrieve_args(
    run_id: u64,
    artifact_id: &str,
    dir: &Path,
    repo: &str,
) -> Result<Vec<OsString>, OrchestratorError> {
    validate_artifact_id(artifact_id).map_err(internal_contract)?;
    let Some(repo) = velnor_actions_orchestrator_core::origin::validate_repository_slug(repo)
    else {
        return Err(internal_contract(
            velnor_actions_contract::ContractError::identity("repository", "bad_lookup_repo"),
        ));
    };
    Ok(vec![
        OsString::from("run"),
        OsString::from("download"),
        OsString::from(run_id.to_string()),
        OsString::from("--name"),
        OsString::from(artifact_id),
        OsString::from("--dir"),
        dir.as_os_str().to_owned(),
        OsString::from("--repo"),
        OsString::from(repo),
    ])
}

/// Maximum bytes read for the retrieve-step plan.
///
/// Matches the merge-request assembly bound: the same `plan.json`
/// parses identically at retrieve and merge time, and a giant plan
/// downloads nothing instead of exhausting the final job's memory.
pub(crate) const MAX_RETRIEVE_PLAN_BYTES: u64 = 4 << 20;

/// Parse the downloaded plan, if any.
///
/// Symlink-rejecting, size-bounded, duplicate-key-rejecting (X7); typed
/// plan structs additionally carry `deny_unknown_fields`. Bounded like
/// every other event-time read: a missing, oversize, or unparsable plan
/// downloads nothing, and the merge still reaches its `planning_failed`
/// verdict.
fn read_plan(run_dir: &Path) -> Option<serde_json::Value> {
    let text = read_staged_text(&run_dir.join("plan.json"), MAX_RETRIEVE_PLAN_BYTES).ok()?;
    parse_strict_json(&text).ok()
}

/// Maximum bytes read from one staged report file (P01-6 size bound).
///
/// Matrix and task reports are kilobytes; one megabyte fails closed on
/// runaway payloads without truncating legitimate evidence.
pub(crate) const MAX_STAGED_REPORT_BYTES: u64 = 1 << 20;

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

/// Expected job artifact IDs from a plan value, in plan order.
///
/// Sibling entries share their job's artifact, so the enumeration
/// dedupes: each artifact downloads exactly once while first-seen
/// plan order stays stable.
fn expected_artifact_ids(plan: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            if let Some(id) = entry.get("artifact_id").and_then(serde_json::Value::as_str)
                && seen.insert(id)
            {
                ids.push(id);
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests;
