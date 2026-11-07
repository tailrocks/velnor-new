// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CheckOutcome, DiscoveredCheck, MAX_CHECK_EXECUTION_RECEIPT_BYTES, MatrixEntry,
    OrchestratorError, Path, Plan, canonical_json_bytes, compiler_observation, internal,
    internal_contract,
};

pub(super) fn write_execution_receipt(
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
    velnor_actions_contract::exclusive_write::create_dir_no_symlink(temp, &base)?;
    velnor_actions_contract::exclusive_write::write_exclusive(
        &base.join("check-execution.json"),
        &bytes,
        "check_execution",
    )?;
    Ok(())
}

pub(super) fn write_observation_report(
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
    velnor_actions_contract::exclusive_write::create_dir_no_symlink(temp, &base)?;
    velnor_actions_contract::exclusive_write::write_exclusive(
        &base.join("check-observation.json"),
        &bytes,
        "check_observation",
    )?;
    Ok(())
}
