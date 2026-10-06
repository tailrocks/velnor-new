//! Plan-bound report payloads: preserve evidence bytes and omit plan authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use velnor_actions_contract::{ExecuteTaskRef, Plan, task_report_id_for_task};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::retrieve_reports::read_staged_bytes;

/// Sealed report-payload staging operation.
pub const STAGE_REPORTS_OP: &str = "stage-reports-v1";
const MAX_FILES: usize = 16_384;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const REPORT_BOUND: u64 = 1 << 20;
const ACTION_BOUND: u64 = 16 * 1024;

/// Copy only existing plan-expected reports into a fresh artifact payload.
///
/// Missing and partial evidence stays missing and partial. Report content is
/// neither decoded nor rewritten: merge remains the evidence validator.
///
/// # Errors
///
/// Rejects invalid plan/run bindings, unsafe paths, seeded payloads, and bounds.
pub fn stage_reports() -> Result<usize, OrchestratorError> {
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    stage_reports_to(&run_key, Path::new(&temp))
}

/// Explicit staging core; the validated downloaded plan supplies all paths.
pub(crate) fn stage_reports_to(
    run_key: &str,
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    velnor_actions_contract::validate_run_key(run_key).map_err(internal_contract)?;
    let source = runner_temp.join("velnor").join(run_key);
    safe_chain(runner_temp, &source.join("plan.json"))?;
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    let expected = expected_paths(&plan)?;
    let files = collect_files(runner_temp, &source, expected)?;
    let destination = runner_temp
        .join("velnor")
        .join("report-payload")
        .join(run_key);
    let parent = destination
        .parent()
        .ok_or_else(|| internal("missing_dir_anchor"))?;
    crate::exclusive_write::create_dir_no_symlink(runner_temp, parent)?;
    fs::create_dir(&destination).map_err(|_| internal("report_payload_exists_or_unwritable"))?;
    safe_chain(runner_temp, &destination)?;
    for (relative, bytes) in &files {
        let path = destination.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| internal("missing_dir_anchor"))?;
        crate::exclusive_write::create_dir_no_symlink(runner_temp, parent)?;
        crate::exclusive_write::write_exclusive(&path, bytes, "report_payload")?;
    }
    Ok(files.len())
}

/// Closed inventory, independent of report contents and directory listings.
fn expected_paths(plan: &Plan) -> Result<BTreeMap<PathBuf, u64>, OrchestratorError> {
    let digests: BTreeMap<_, _> = plan
        .obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect();
    let mut paths = BTreeMap::new();
    for entry in &plan.matrix.include {
        let home = PathBuf::from(&entry.matrix_key);
        paths.insert(home.join("matrix-report.json"), REPORT_BOUND);
        let mut ids = BTreeSet::new();
        for reference in entry.execute_task_ids.tasks.values() {
            match reference {
                ExecuteTaskRef::Single(id) => {
                    ids.insert(id.as_str());
                }
                ExecuteTaskRef::Shards(shards) => {
                    ids.extend(shards.iter().map(String::as_str));
                }
            }
        }
        for id in ids {
            let digest = digests
                .get(id)
                .ok_or_else(|| internal("task_without_obligation"))?;
            let report = task_report_id_for_task(&plan.run_key, &entry.matrix_key, digest)
                .map_err(internal_contract)?;
            paths.insert(
                home.join("tasks").join(format!("{report}.json")),
                REPORT_BOUND,
            );
        }
        if crate::merge::action_admission::executes_action(plan, entry) {
            paths.insert(home.join("actions/begin.json"), ACTION_BOUND);
            paths.insert(home.join("actions/report.json"), ACTION_BOUND);
        }
        if crate::merge::helper_admission::executes_helper(plan, entry) {
            paths.insert(home.join("helpers/begin.json"), REPORT_BOUND);
            paths.insert(home.join("helpers/report.json"), REPORT_BOUND);
        }
        if paths.len() > MAX_FILES {
            return Err(internal("report_payload_file_bound"));
        }
    }
    Ok(paths)
}

/// Read exact evidence bytes before creating any upload payload.
fn collect_files(
    anchor: &Path,
    source: &Path,
    expected: BTreeMap<PathBuf, u64>,
) -> Result<Vec<(PathBuf, Vec<u8>)>, OrchestratorError> {
    let mut files = Vec::new();
    let mut total = 0usize;
    for (relative, bound) in expected {
        let path = source.join(&relative);
        if !safe_chain(anchor, &path)? {
            continue;
        }
        let bytes = match read_staged_bytes(&path, bound) {
            Ok(bytes) => bytes,
            Err("missing") => continue,
            Err(kind) => return Err(internal(&format!("report_payload_{kind}"))),
        };
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| internal("report_payload_byte_bound"))?;
        if total > MAX_BYTES {
            return Err(internal("report_payload_byte_bound"));
        }
        files.push((relative, bytes));
    }
    Ok(files)
}

/// Refuse every symlink/component escape, including source ancestors.
fn safe_chain(anchor: &Path, path: &Path) -> Result<bool, OrchestratorError> {
    let relative = path
        .strip_prefix(anchor)
        .map_err(|_| internal("anchor_escape"))?;
    let mut current = anchor.to_path_buf();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return Err(internal("anchor_escape"));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(internal("symlink_refused")),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(internal("report_payload_unreadable")),
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "report_staging_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "report_staging_helper_tests.rs"]
mod helper_tests;
