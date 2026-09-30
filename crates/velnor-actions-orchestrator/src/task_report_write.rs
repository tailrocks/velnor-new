//! Exclusive report-file writes for the task-report producer.
//!
//! Split from `task_report` (size gate): one entry's matrix report plus
//! its task file land as canonical JSON under the run directory; a
//! pre-existing file errors, never overwrites.

use std::fs;
use std::path::Path;

use velnor_actions_contract::{MatrixEntry, MatrixReport, Plan, TaskReport, canonical_json_bytes};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Write one entry's matrix report plus its task file as canonical JSON.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures and
/// [`OrchestratorError::Io`] for unwritable directories; pre-existing
/// files error, never overwrite.
pub(crate) fn write_entry_reports(
    runner_temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    task: &TaskReport,
    matrix: &MatrixReport,
) -> Result<(), OrchestratorError> {
    let dir = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let task_files_dir = dir.join("tasks");
    fs::create_dir_all(&task_files_dir).map_err(|err| {
        OrchestratorError::io(task_files_dir.display().to_string(), err.to_string())
    })?;
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    let task_bytes = canonical_json_bytes(task).map_err(internal_contract)?;
    write_new(&dir.join("matrix-report.json"), &matrix_bytes)?;
    write_new(
        &task_files_dir.join(format!("{}.json", task.task_report_id)),
        &task_bytes,
    )?;
    Ok(())
}

/// Exclusively write one report file; a pre-existing file errors.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal("report_exists"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(bytes)
                .map_err(|_| internal("report_unwritable"))
        })
}
