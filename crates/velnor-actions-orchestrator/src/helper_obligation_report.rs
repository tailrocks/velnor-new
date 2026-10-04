//! Failure-closed begin and terminal evidence for compiled native obligations.

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    HelperObligationBinding, HelperObligationOutcome, HelperObligationReport, MatrixEntry, Plan,
    canonical_json_bytes, parse_strict_json, validate_digest,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::task_report::{entry_and_digest, load_plan};

/// Private begin operation.
pub const HELPER_BEGIN_OP: &str = "begin-helper-obligation-report-v1";
/// Private terminal operation.
pub const HELPER_REPORT_OP: &str = "write-helper-obligation-report-v1";
/// Deterministic native helper step ID.
pub(crate) const HELPER_ID_ENV: &str = "VELNOR_HELPER_ID";
/// Original GitHub step outcome.
pub(crate) const HELPER_OUTCOME_ENV: &str = "VELNOR_HELPER_OUTCOME";

/// Record the qualified native binding before execution.
/// # Errors
/// Rejects unqualified tasks and pre-existing evidence.
pub fn begin_helper_obligation_report() -> Result<usize, OrchestratorError> {
    let frame = environment()?;
    begin_helper_obligation_report_to(
        &frame.run,
        &frame.task,
        &frame.helper,
        &frame.digest,
        &frame.temp,
    )
}

/// Record terminal native evidence and non-reusable obligation coverage.
/// # Errors
/// Rejects missing/mismatched begin records and non-success after reporting them.
pub fn write_helper_obligation_report() -> Result<usize, OrchestratorError> {
    let frame = environment()?;
    let outcome = match required_env(HELPER_OUTCOME_ENV)?.as_str() {
        "success" => HelperObligationOutcome::Success,
        "failure" => HelperObligationOutcome::Failure,
        "cancelled" => HelperObligationOutcome::Cancelled,
        "skipped" => HelperObligationOutcome::Skipped,
        _ => return Err(internal("invalid_helper_outcome")),
    };
    let downstream = env::var(crate::task_report::DOWNSTREAM_IDS_ENV).unwrap_or_default();
    let downstream: Vec<String> = downstream
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    write_helper_obligation_report_to(
        &frame.run,
        &frame.task,
        &frame.helper,
        &frame.digest,
        outcome,
        &downstream,
        &frame.temp,
    )
}

/// Baked source-owner frame identity required by both runtime operations.
struct ReportEnvironment {
    run: String,
    task: String,
    helper: String,
    digest: String,
    temp: PathBuf,
}

/// Resolve private runtime identities; execution metadata comes from the plan.
fn environment() -> Result<ReportEnvironment, OrchestratorError> {
    let run = crate::internal_request::resolve_run_key(None)?;
    let task = required_env(crate::task_report::TASK_ID_ENV)?;
    let helper = required_env(HELPER_ID_ENV)?;
    let digest = required_env(crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV)?;
    validate_digest(&digest).map_err(internal_contract)?;
    let temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    Ok(ReportEnvironment {
        run,
        task,
        helper,
        digest,
        temp,
    })
}

/// Require a nonempty private runtime value.
fn required_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_helper_report_environment"))
}

/// Testable producer; baseline-covered tasks have no execution evidence.
pub(crate) fn begin_helper_obligation_report_to(
    run: &str,
    task: &str,
    helper: &str,
    framed_digest: &str,
    temp: &Path,
) -> Result<usize, OrchestratorError> {
    let plan = load_plan(run, temp)?;
    validate_frame(&plan, task, framed_digest)?;
    if crate::covered_tasks::covered_by_baseline(&plan, task) {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task)?;
    let expected = binding(&plan, entry, digest, helper)?;
    let dir = helper_dir(temp, &plan, entry);
    crate::exclusive_write::create_dir_no_symlink(temp, &dir)?;
    let bytes = canonical_json_bytes(&expected).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(&dir.join("begin.json"), &bytes, "helper_begin")?;
    Ok(1)
}

/// Terminal producer requires exact equality with the pre-execution binding.
pub(crate) fn write_helper_obligation_report_to(
    run: &str,
    task: &str,
    helper: &str,
    framed_digest: &str,
    outcome: HelperObligationOutcome,
    downstream: &[String],
    temp: &Path,
) -> Result<usize, OrchestratorError> {
    let plan = load_plan(run, temp)?;
    validate_frame(&plan, task, framed_digest)?;
    if crate::covered_tasks::covered_by_baseline(&plan, task) {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task)?;
    let expected = binding(&plan, entry, digest, helper)?;
    let dir = helper_dir(temp, &plan, entry);
    crate::exclusive_write::create_dir_no_symlink(temp, &dir)?;
    let before = read_begin(&dir.join("begin.json"))?;
    if before != expected {
        return Err(internal("helper_begin_mismatch"));
    }
    let report = HelperObligationReport {
        binding: before,
        outcome,
    };
    report.validate().map_err(internal_contract)?;
    let bytes = canonical_json_bytes(&report).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(&dir.join("report.json"), &bytes, "helper_report")?;
    crate::helper_obligation_coverage::write(&plan, entry, digest, outcome, temp)?;
    if outcome != HelperObligationOutcome::Success {
        crate::helper_obligation_coverage::downstream(&plan, entry, downstream, temp)?;
        return Err(internal("helper_not_successful"));
    }
    Ok(1)
}

/// A fresh plan cannot authorize a helper baked from another task identity.
fn validate_frame(plan: &Plan, task: &str, framed_digest: &str) -> Result<(), OrchestratorError> {
    validate_digest(framed_digest).map_err(internal_contract)?;
    let planned = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task)
        .ok_or_else(|| internal("helper_frame_task_missing"))?;
    if planned.task_digest != framed_digest {
        return Err(internal("helper_frame_task_digest_mismatch"));
    }
    Ok(())
}

/// Reconstruct compiled owner authority; never trust serialized invocation alone.
pub(crate) fn binding(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    helper: &str,
) -> Result<HelperObligationBinding, OrchestratorError> {
    let record = crate::helper_obligation_binding::record_for_entry(plan, entry)?;
    let expected_descriptor =
        crate::helper_obligation_steps::descriptor(&record, &entry.matrix_key)?;
    if entry.adapter_metadata.get("helper_obligation") != Some(&expected_descriptor) {
        return Err(internal("helper_descriptor_mismatch"));
    }
    let binding = HelperObligationBinding {
        schema: 1,
        run_key: plan.run_key.clone(),
        source_head: plan.head.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        task_digest: digest.to_owned(),
        helper_id: helper.to_owned(),
        invocation: record.invocation().clone(),
        environment: record.environment().clone(),
    };
    binding.validate().map_err(internal_contract)?;
    Ok(binding)
}

/// Strict bounded, symlink-refusing evidence read.
fn read_begin(path: &Path) -> Result<HelperObligationBinding, OrchestratorError> {
    let text = crate::retrieve_reports::read_staged_text(path, 1024 * 1024)
        .map_err(|kind| internal(&format!("invalid_helper_begin:{kind}")))?;
    let value = parse_strict_json(&text).map_err(|_| internal("invalid_helper_begin"))?;
    let binding: HelperObligationBinding =
        serde_json::from_value(value).map_err(|_| internal("invalid_helper_begin"))?;
    binding.validate().map_err(internal_contract)?;
    Ok(binding)
}

/// Each helper report belongs to one planned obligation.
fn helper_dir(temp: &Path, plan: &Plan, entry: &MatrixEntry) -> PathBuf {
    temp.join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
        .join("helpers")
}

#[cfg(test)]
#[path = "helper_obligation_report_tests.rs"]
mod tests;
