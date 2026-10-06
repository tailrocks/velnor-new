//! Failure-closed begin/after evidence for the closed Docker action obligation.

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    ActionBinding, ActionOutcome, ActionReport, MatrixEntry, NotSelectedReason, Plan, TaskStatus,
    canonical_json_bytes, parse_strict_json,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::task_report::{entry_and_digest, load_plan, single_task_aggregate, write_entry_reports};

/// Private begin operation.
pub const ACTION_BEGIN_OP: &str = "begin-action-report-v1";
/// Private terminal operation.
pub const ACTION_REPORT_OP: &str = "write-action-report-v1";
/// Logical validation action ID, separate from its later cache export.
pub(crate) const ACTION_ID_ENV: &str = "VELNOR_ACTION_ID";
/// Upstream action outcome, captured by GitHub's step context.
pub(crate) const ACTION_OUTCOME_ENV: &str = "VELNOR_ACTION_OUTCOME";

/// Persist the planned action binding before execution.
/// # Errors
/// Rejects unknown tasks, unsupported actions, and pre-existing evidence.
pub fn begin_action_report() -> Result<usize, OrchestratorError> {
    let (run, task, action, temp) = environment()?;
    begin_action_report_to(&run, &task, &action, &temp)
}

/// Persist terminal Action API evidence and ordinary obligation coverage.
/// # Errors
/// Rejects missing/mismatched begins and non-success outcomes, after reporting them.
pub fn write_action_report() -> Result<usize, OrchestratorError> {
    let (run, task, action, temp) = environment()?;
    let outcome = match required_env(ACTION_OUTCOME_ENV)?.as_str() {
        "success" => ActionOutcome::Success,
        "failure" => ActionOutcome::Failure,
        "cancelled" => ActionOutcome::Cancelled,
        "skipped" => ActionOutcome::Skipped,
        _ => return Err(internal("invalid_action_outcome")),
    };
    write_action_report_to(&run, &task, &action, outcome, &temp)
}

/// Resolve the private producer environment without trusting an action ref.
fn environment() -> Result<(String, String, String, PathBuf), OrchestratorError> {
    let run = crate::internal_request::resolve_run_key(None)?;
    let task = required_env(crate::task_report::TASK_ID_ENV)?;
    let action = required_env(ACTION_ID_ENV)?;
    let temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    Ok((run, task, action, temp))
}

/// Require a nonempty private value.
fn required_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_action_report_environment"))
}

/// Testable begin producer; covered tasks have no executable action.
pub(crate) fn begin_action_report_to(
    run: &str,
    task: &str,
    action: &str,
    temp: &Path,
) -> Result<usize, OrchestratorError> {
    let plan = load_plan(run, temp)?;
    if crate::covered_tasks::covered_by_baseline(&plan, task) {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task)?;
    let binding = binding(&plan, entry, digest, action)?;
    let dir = action_dir(temp, &plan, entry);
    crate::exclusive_write::create_dir_no_symlink(temp, &dir)?;
    let bytes = canonical_json_bytes(&binding).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(&dir.join("begin.json"), &bytes, "action_begin")?;
    Ok(1)
}

/// Testable terminal producer; success requires the same pre-action binding.
pub(crate) fn write_action_report_to(
    run: &str,
    task: &str,
    action: &str,
    outcome: ActionOutcome,
    temp: &Path,
) -> Result<usize, OrchestratorError> {
    let plan = load_plan(run, temp)?;
    if crate::covered_tasks::covered_by_baseline(&plan, task) {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task)?;
    let expected = binding(&plan, entry, digest, action)?;
    let dir = action_dir(temp, &plan, entry);
    crate::exclusive_write::create_dir_no_symlink(temp, &dir)?;
    let before = read_begin(&dir.join("begin.json"))?;
    if before != expected {
        return Err(internal("action_begin_mismatch"));
    }
    let report = ActionReport {
        binding: before,
        outcome,
    };
    report.validate().map_err(internal_contract)?;
    let bytes = canonical_json_bytes(&report).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(&dir.join("report.json"), &bytes, "action_report")?;
    write_coverage(&plan, entry, digest, outcome, temp)?;
    if outcome != ActionOutcome::Success {
        return Err(internal("action_not_successful"));
    }
    Ok(1)
}

/// Bind only the closed Docker build obligation and its qualified action pin.
pub(crate) fn binding(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    action: &str,
) -> Result<ActionBinding, OrchestratorError> {
    if entry.stack_id != "workload"
        || entry
            .adapter_metadata
            .get("configuration")
            .and_then(serde_json::Value::as_str)
            != Some("docker_build")
        || entry
            .adapter_metadata
            .get("kind")
            .and_then(serde_json::Value::as_str)
            != Some("build")
    {
        return Err(internal("unsupported_action_obligation"));
    }
    let binding = ActionBinding {
        schema: 1,
        run_key: plan.run_key.clone(),
        source_head: plan.head.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        task_digest: digest.to_owned(),
        action_id: action.to_owned(),
        action_ref: format!(
            "docker/build-push-action@{}",
            velnor_actions_actionlint::actions::BUILD_PUSH_ACTION_SHA
        ),
    };
    binding.validate().map_err(internal_contract)?;
    let descriptor = entry
        .adapter_metadata
        .get("action")
        .ok_or_else(|| internal("missing_action_descriptor"))?;
    if descriptor
        .as_object()
        .is_none_or(|object| object.len() != 2)
        || descriptor.get("id").and_then(serde_json::Value::as_str)
            != Some(binding.action_id.as_str())
        || descriptor.get("uses").and_then(serde_json::Value::as_str)
            != Some(binding.action_ref.as_str())
    {
        return Err(internal("action_descriptor_mismatch"));
    }
    Ok(binding)
}

/// Read bounded, strict, symlink-refusing begin evidence.
fn read_begin(path: &Path) -> Result<ActionBinding, OrchestratorError> {
    let text = crate::retrieve_reports::read_staged_text(path, 16 * 1024)
        .map_err(|kind| internal(&format!("invalid_action_begin:{kind}")))?;
    let value = parse_strict_json(&text).map_err(|_| internal("invalid_action_begin"))?;
    let binding: ActionBinding =
        serde_json::from_value(value).map_err(|_| internal("invalid_action_begin"))?;
    binding.validate().map_err(internal_contract)?;
    Ok(binding)
}

/// Coverage remains non-reusable and carries no fabricated timing or outputs.
fn write_coverage(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    outcome: ActionOutcome,
    temp: &Path,
) -> Result<(), OrchestratorError> {
    let exit_code = match outcome {
        ActionOutcome::Success => 0,
        ActionOutcome::Cancelled => 130,
        ActionOutcome::Failure | ActionOutcome::Skipped => 1,
    };
    let mut task = crate::task_report::terminal_task_report(plan, entry, digest, exit_code, None)
        .map_err(internal_contract)?;
    match outcome {
        ActionOutcome::Cancelled => task.status = TaskStatus::Cancelled,
        ActionOutcome::Skipped => {
            task.status = TaskStatus::NotSelected;
            task.not_selected_reason = Some(NotSelectedReason::UpstreamFailed);
        }
        ActionOutcome::Success | ActionOutcome::Failure => {}
    }
    task.validate().map_err(internal_contract)?;
    let matrix = single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(temp, plan, entry, &task, &matrix)
}

/// Action evidence belongs to its single planned matrix entry.
fn action_dir(temp: &Path, plan: &Plan, entry: &MatrixEntry) -> PathBuf {
    temp.join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
        .join("actions")
}

#[cfg(test)]
#[path = "action_report_tests.rs"]
mod tests;
