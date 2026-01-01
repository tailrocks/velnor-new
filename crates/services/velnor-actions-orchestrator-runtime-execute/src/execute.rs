//! Qualified execution driver for one named check.
//!
//! Loads the staged plan and source configuration, binds the
//! discovered check to the planner's derivation, runs it under its
//! deadline, then publishes the ordinary task and matrix reports
//! plus the execution receipt. Identity comes from the caller; this
//! module never reads the process environment.

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use velnor_actions_contract_workflow::Plan;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, ToolCatalog, discover_checks_until};
use velnor_actions_orchestrator_check_evidence::scenario::verify_evidence;
use velnor_actions_orchestrator_check_preparation::preparation::container;
use velnor_actions_orchestrator_check_preparation::preparation::prepare_check;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::link_safety::reject_link_components;
use velnor_actions_orchestrator_core::{internal, internal_contract};
use velnor_actions_orchestrator_runtime_evidence::evidence::CheckOutcome;

/// Check lookup key; source configuration and plan independently bind it.
pub const CHECK_ID_ENV: &str = "VELNOR_CHECK_ID";

/// Execute the bound check and publish its ordinary task/matrix reports.
///
/// The caller supplies every identity (run key, check, task, job,
/// lane) plus the shared plan byte bound both `plan.json` readers
/// honor.
///
/// # Errors
///
/// Missing source binding, capabilities, task failures, invalid
/// evidence, and unwritable report paths fail closed.
pub fn execute_check_to(
    root: &Path,
    temp: &Path,
    run_key: &str,
    id: &str,
    task_id: &str,
    job_id: &str,
    lane: &str,
    max_plan_bytes: u64,
) -> Result<usize, OrchestratorError> {
    let started = Instant::now();
    let plan = velnor_actions_orchestrator_task_report::task_report::load_plan(
        run_key,
        temp,
        max_plan_bytes,
    )?;
    let config = velnor_actions_orchestrator_core::config::load_config(root)?;
    let definition = config
        .checks
        .iter()
        .find(|check| check.id == id)
        .ok_or_else(|| internal("check_not_configured"))?;
    let deadline = CheckDeadline::from_start(
        started,
        Duration::from_secs(u64::from(definition.timeout_minutes) * 60),
    )
    .map_err(|e| internal(&e.to_string()))?;
    velnor_actions_orchestrator_selection::select::verify_checkout_until(
        root, plan.event, &plan.head, deadline,
    )?;
    let catalog = ToolCatalog::pinned();
    let mut discovered = discover_checks_until(
        root,
        std::slice::from_ref(definition),
        &config.qualified_tools,
        deadline,
    )
    .map_err(|e| internal(&e.to_string()))?;
    let item = discovered
        .pop()
        .ok_or_else(|| internal("check_not_discovered"))?;
    let (entry, digest) =
        velnor_actions_orchestrator_task_report::task_report::entry_and_digest_for_job(
            &plan, task_id, job_id,
        )?;
    let lane_variant = velnor_actions_orchestrator_runtime_plan::binding::parse_lane_variant(lane)?;
    if entry.lane_variant != lane_variant {
        return Err(internal("check_lane_variant_mismatch"));
    }
    velnor_actions_orchestrator_runtime_plan::binding::bind_check(
        root, &item, &plan, entry, task_id, &catalog, deadline,
    )?;
    let outcome = run_check(root, temp, &item, &plan, deadline);
    let (code, receipt) = match &outcome {
        Ok(success) => (0, success.evidence.as_ref()),
        Err(_) => (1, None),
    };
    let duration = u64::try_from(started.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let mut task = velnor_actions_orchestrator_task_report::task_report::terminal_task_report(
        &plan,
        entry,
        digest,
        code,
        Some(duration),
    )
    .map_err(internal_contract)?;
    if let Some(receipt) = receipt {
        velnor_actions_orchestrator_runtime_evidence::evidence::save_evidence(
            temp, &plan, entry, receipt,
        )?;
        task.outputs.push(receipt.path.clone());
    }
    if let Ok(success) = &outcome {
        velnor_actions_orchestrator_runtime_evidence::evidence::write_execution_receipt(
            temp, &plan, entry, &item, success,
        )?;
    }
    task.validate_outputs_declared(&entry.declared_outputs)
        .map_err(internal_contract)?;
    let matrix = velnor_actions_orchestrator_task_report::task_report::single_task_aggregate(
        &plan, entry, &task,
    )
    .map_err(internal_contract)?;
    velnor_actions_orchestrator_task_report::task_report::write_entry_reports(
        temp, &plan, entry, &task, &matrix,
    )?;
    outcome.map(|_| 1)
}

fn run_check(
    root: &Path,
    temp: &Path,
    item: &DiscoveredCheck,
    plan: &Plan,
    deadline: CheckDeadline,
) -> Result<CheckOutcome, OrchestratorError> {
    deadline.remaining().map_err(|e| internal(&e.to_string()))?;
    if let Some(evidence) = &item.check.evidence {
        reject_link_components(root, &evidence.path)?;
        if std::fs::symlink_metadata(root.join(&evidence.path)).is_ok() {
            return Err(internal("check_evidence_preexisting"));
        }
    }
    let owned = prepare_check(root, temp, item, deadline)?;
    let platform =
        serde_json::to_value(item.check.runner.platform).map_err(|_| internal("check_platform"))?;
    let pairs = [
        (
            OsString::from("VELNOR_CHECK_HEAD"),
            OsString::from(&plan.head),
        ),
        (OsString::from(CHECK_ID_ENV), OsString::from(&item.check.id)),
        (
            OsString::from("VELNOR_CHECK_PLATFORM"),
            OsString::from(
                platform
                    .as_str()
                    .ok_or_else(|| internal("check_platform"))?,
            ),
        ),
    ];
    let before = container::probe(&item.check.runner, owned.container.as_ref(), deadline)?;
    let command = owned
        .command(deadline, velnor_actions_mise::MISE_VERSION)
        .map_err(|e| internal(&e.to_string()))?
        .with_env(&pairs)
        .map_err(|e| internal(&e.to_string()))?;
    let task_output = command.run_until(8 * 1024 * 1024, deadline);
    let after_result = container::probe(&item.check.runner, owned.container.as_ref(), deadline);
    let output = task_output.map_err(|e| internal(&e.to_string()))?;
    let after = after_result?;
    let container =
        container::receipt(&item.check.runner, owned.container.as_ref(), before, after)?;
    output
        .require_success("mise-check")
        .map_err(|e| internal(&e.to_string()))?;
    let evidence = item
        .check
        .evidence
        .as_ref()
        .map(|evidence| {
            verify_evidence(
                root,
                evidence,
                &item.check.id,
                &plan.head,
                item.check.runner.platform,
            )
        })
        .transpose()?;
    Ok(CheckOutcome {
        evidence,
        container,
        system_tools: owned.system_tools.clone(),
        qualified_tools: owned.qualified_tools.clone(),
    })
}
