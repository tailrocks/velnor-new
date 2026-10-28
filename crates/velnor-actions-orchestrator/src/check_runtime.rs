// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::check_evidence::{EvidenceReceipt, reject_link_components, verify_evidence};
use crate::internal::{internal, internal_contract};
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};
use velnor_actions_contract::config::MAX_CHECK_EXECUTION_RECEIPT_BYTES;
use velnor_actions_contract::{
    MatrixEntry, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NamedCheckLane,
    NamedCheckLaneVariant, ObligationDecision, Plan, canonical_json_bytes,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, discover_checks_until};
#[path = "check_prepare.rs"]
pub(crate) mod preparation;
use preparation::{
    RestoredGroupContext, prepare_check_with_runtime, resolve_mise_runtime,
};
use velnor_actions_mise::catalog::lock::ApprovedMiseRuntime;
#[path = "check_binding.rs"]
mod check_binding;
use check_binding::CheckBinding;
#[path = "compiler_observation.rs"]
mod compiler_observation;

pub const EXECUTE_CHECK_OP: &str = "execute-check-v1";
pub(crate) const CHECK_ID_ENV: &str = "VELNOR_CHECK_ID";

pub fn execute_check() -> Result<usize, OrchestratorError> {
    let root = env::var_os("GITHUB_WORKSPACE")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_check_workspace"))?;
    let temp = env::var_os("RUNNER_TEMP")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let id = required_env(CHECK_ID_ENV)?;
    let task_id = required_env(crate::task_report::TASK_ID_ENV)?;
    let job_id = required_env(NAMED_CHECK_JOB_ID_ENV)?;
    let lane = required_env(NAMED_CHECK_LANE_VARIANT_ENV)?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    execute_check_to(
        Path::new(&root),
        Path::new(&temp),
        &run_key,
        &id,
        &task_id,
        &job_id,
        &lane,
    )
}
fn required_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_check_identity"))
}

pub(crate) fn execute_check_to(
    root: &Path,
    temp: &Path,
    run_key: &str,
    id: &str,
    task_id: &str,
    job_id: &str,
    lane: &str,
) -> Result<usize, OrchestratorError> {
    let started = Instant::now();
    let plan = crate::task_report::load_plan(run_key, temp)?;
    let config = crate::config::load_config(root)?;
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
    crate::select::verify_checkout_until(root, plan.event, &plan.head, deadline)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(root, &config)?;
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
    let (entry, digest) = crate::task_report::entry_and_digest_for_job(&plan, task_id, job_id)?;
    let lane_variant = parse_lane_variant(lane)?;
    if entry.lane_variant != lane_variant {
        return Err(internal("check_lane_variant_mismatch"));
    }
    let runtime = resolve_mise_runtime(root, config.workflow.policy, &item, deadline)?;
    bind_check(CheckBinding {
        root,
        item: &item,
        plan: &plan,
        entry,
        task_id,
        catalog: &catalog,
        runtime: &runtime,
        deadline,
    })?;
    let outcome = run_check(
        root,
        temp,
        &item,
        &plan,
        entry,
        &runtime,
        deadline,
    );
    let (code, receipt) = match &outcome {
        Ok(success) => (0, success.evidence.as_ref()),
        Err(_) => (1, None),
    };
    let duration = u64::try_from(started.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let mut task =
        crate::task_report::terminal_task_report(&plan, entry, digest, code, Some(duration))
            .map_err(internal_contract)?;
    if let Some(receipt) = receipt {
        save_evidence(temp, &plan, entry, receipt)?;
        task.outputs.push(receipt.path.clone());
    }
    if let Ok(success) = &outcome {
        write_execution_receipt(temp, &plan, entry, &item, success)?;
    }
    task.validate_outputs_declared(&entry.declared_outputs)
        .map_err(internal_contract)?;
    let matrix = crate::task_report::single_task_aggregate(&plan, entry, &task)
        .map_err(internal_contract)?;
    crate::task_report::write_entry_reports(temp, &plan, entry, &task, &matrix)?;
    outcome.map(|_| 1)
}

fn bind_check(binding: CheckBinding<'_>) -> Result<(), OrchestratorError> {
    let CheckBinding {
        root,
        item,
        plan,
        entry,
        task_id,
        catalog,
        runtime,
        deadline,
    } = binding;
    if item.proposal.task_id != task_id {
        return Err(internal("check_task_identity"));
    }
    if item.check.runner.platform.os() != std::env::consts::OS
        || item.check.runner.platform.arch() != std::env::consts::ARCH
    {
        return Err(internal("check_host_platform"));
    }
    for input in &item.config_inputs {
        reject_link_components(root, input)?;
    }
    let lane = NamedCheckLane {
        variant: entry.lane_variant,
        job_id: entry.job_id.clone(),
    };
    let (expected, mut expected_entries) =
        crate::internal_plan::named_checks::plan::derive_lanes_until(
            root,
            item,
            &plan.run_key,
            &plan.generator,
            crate::internal_plan::named_checks::plan::RuntimeToolchain { catalog, runtime },
            &[lane],
            Some(deadline),
        )?;
    let expected_entry = expected_entries
        .pop()
        .ok_or_else(|| internal("named_check_lane_missing"))?;
    let actual = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == task_id)
        .ok_or_else(|| internal("check_obligation_missing"))?;
    if actual.decision != ObligationDecision::Execute
        || actual.task_digest != expected.task_digest
        || actual.input_digest != expected.input_digest
        || actual.closure_digest != expected.closure_digest
        || canonical_json_bytes(entry).map_err(internal_contract)?
            != canonical_json_bytes(&expected_entry).map_err(internal_contract)?
    {
        return Err(internal("check_plan_identity"));
    }
    Ok(())
}

fn parse_lane_variant(value: &str) -> Result<Option<NamedCheckLaneVariant>, OrchestratorError> {
    match value {
        "single" => Ok(None),
        "hosted" => Ok(Some(NamedCheckLaneVariant::Hosted)),
        "scale_set" => Ok(Some(NamedCheckLaneVariant::ScaleSet)),
        _ => Err(internal("check_lane_variant_invalid")),
    }
}

struct CheckOutcome {
    evidence: Option<EvidenceReceipt>,
    container: Option<crate::check_evidence::gate::container::ContainerReceipt>,
    system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
}

struct ChildExecution {
    evidence: Option<EvidenceReceipt>,
    container: Option<crate::check_evidence::gate::container::ContainerReceipt>,
    system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
}

fn run_check(
    root: &Path,
    temp: &Path,
    item: &DiscoveredCheck,
    plan: &Plan,
    entry: &MatrixEntry,
    runtime: &ApprovedMiseRuntime,
    deadline: CheckDeadline,
) -> Result<CheckOutcome, OrchestratorError> {
    deadline.remaining().map_err(|e| internal(&e.to_string()))?;
    if let Some(evidence) = &item.check.evidence {
        reject_link_components(root, &evidence.path)?;
        if std::fs::symlink_metadata(root.join(&evidence.path)).is_ok() {
            return Err(internal("check_evidence_preexisting"));
        }
    }
    let group = RestoredGroupContext::resolve(temp)?;
    let mut owned = prepare_check_with_runtime(root, &group, item, runtime, deadline)?;
    let executed = run_owned(root, item, plan, deadline, &owned);
    let outcome = if executed.is_err() {
        compiler_observation::ChildOutcome::Failure
    } else {
        compiler_observation::ChildOutcome::Success
    };
    let observation = owned.finish_observation(outcome)?;
    write_observation_report(temp, plan, entry, &observation)?;
    let executed = executed?;
    Ok(CheckOutcome {
        evidence: executed.evidence,
        container: executed.container,
        system_tools: executed.system_tools,
        qualified_tools: executed.qualified_tools,
    })
}

fn run_owned(
    root: &Path,
    item: &DiscoveredCheck,
    plan: &Plan,
    deadline: CheckDeadline,
    owned: &preparation::OwnedCheck,
) -> Result<ChildExecution, OrchestratorError> {
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
    let before =
        preparation::container::probe(&item.check.runner, owned.container.as_ref(), deadline)?;
    let command = owned
        .command(deadline)
        .map_err(|e| internal(&e.to_string()))?
        .with_env(&pairs)
        .map_err(|e| internal(&e.to_string()))?;
    let task_output = command.run_until(8 * 1024 * 1024, deadline);
    let after_result =
        preparation::container::probe(&item.check.runner, owned.container.as_ref(), deadline);
    let output = task_output.map_err(|e| internal(&e.to_string()))?;
    let after = after_result?;
    let container = preparation::container::receipt(
        &item.check.runner,
        owned.container.as_ref(),
        before,
        after,
    )?;
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
    Ok(ChildExecution {
        evidence,
        container,
        system_tools: owned.system_tools.clone(),
        qualified_tools: owned.qualified_tools.clone(),
    })
}

fn write_execution_receipt(
    temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    item: &DiscoveredCheck,
    outcome: &CheckOutcome,
) -> Result<(), OrchestratorError> {
    let mut execution = crate::check_evidence::gate::execution_receipt(
        plan,
        entry,
        &item.check.id,
        item.check.runner.platform,
        outcome.evidence.clone(),
        outcome.system_tools.clone(),
        outcome.qualified_tools.clone(),
    );
    execution.container.clone_from(&outcome.container);
    let bytes = canonical_json_bytes(&execution).map_err(internal_contract)?;
    if bytes.len() > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
        return Err(internal("check_execution_receipt_size_limit"));
    }
    let base = temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    crate::exclusive_write::create_dir_no_symlink(temp, &base)?;
    crate::exclusive_write::write_exclusive(
        &base.join("check-execution.json"),
        &bytes,
        "check_execution",
    )
}

fn write_observation_report(
    temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    observation: &compiler_observation::ObservationReport,
) -> Result<(), OrchestratorError> {
    let bytes = observation
        .to_canonical_json()
        .map_err(|error| internal(&error.to_string()))?;
    let base = temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    crate::exclusive_write::create_dir_no_symlink(temp, &base)?;
    crate::exclusive_write::write_exclusive(
        &base.join("check-observation.json"),
        &bytes,
        "check_observation",
    )
}

fn save_evidence(
    temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    receipt: &EvidenceReceipt,
) -> Result<(), OrchestratorError> {
    let base = temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let artifact = base.join("evidence").join(&receipt.path);
    let parent = artifact
        .parent()
        .ok_or_else(|| internal("check_evidence_path"))?;
    crate::exclusive_write::create_dir_no_symlink(temp, parent)?;
    crate::exclusive_write::write_exclusive(&artifact, &receipt.bytes, "check_evidence")?;
    Ok(())
}
