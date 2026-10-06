//! Admit helper authority only after fresh source-qualified identity equality.

use velnor_actions_contract::{CompiledSourceHelper, MatrixEntry, Plan};

use crate::OrchestratorError;
use crate::current_semantic_input_proof::CurrentSemanticInputProof;
use crate::internal::internal;

/// Compare every recorded execution dimension to the actual checkout's proposal.
pub(super) fn record_for_entry(
    plan: &Plan,
    entry: &MatrixEntry,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let proof = CurrentSemanticInputProof::acquire(plan, entry)?;
    let generator = proof.generator();
    if proof.task().task_id != entry.task_id
        || proof.task().stack_id != entry.stack_id
        || proof.run_key() != plan.run_key
        || proof.runner_label() != plan.runner.label
        || generator.version != plan.generator.version
        || generator.target != plan.generator.target
        || generator.sha256 != plan.generator.sha256
        || !proof.root().is_absolute()
    {
        return Err(internal("helper_source_context_mismatch"));
    }
    let actual = proof.identity();
    let expected_tasks = serde_json::to_value(crate::internal_plan::execute_ids(proof.task()))
        .map_err(|_| internal("helper_source_tasks_invalid"))?;
    if serde_json::to_value(&entry.execute_task_ids)
        .map_err(|_| internal("helper_source_tasks_invalid"))?
        != expected_tasks
        || entry.declared_outputs != proof.task().outputs
        || !entry.test_run.is_empty()
    {
        return Err(internal("helper_source_tasks_mismatch"));
    }
    let obligation = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == entry.task_id)
        .ok_or_else(|| internal("helper_source_obligation_missing"))?;
    let expected_run = velnor_actions_workflow_renderer::join_argv_for_run(&actual.argv)?;
    if entry.run != expected_run
        || entry.task_digest != actual.task_digest
        || entry.input_digest != actual.input_digest
        || obligation.task_digest != actual.task_digest
        || obligation.input_digest != actual.input_digest
        || obligation.closure_digest != actual.closure_digest
        || obligation.execution_identity != actual.execution_identity
        || entry.native_recipe != actual.native_recipe
    {
        return Err(internal("helper_source_identity_mismatch"));
    }
    let descriptor = actual
        .helper_obligation
        .as_ref()
        .ok_or_else(|| internal("helper_source_descriptor_missing"))?;
    let expected = serde_json::to_value(descriptor)
        .map_err(|_| internal("helper_source_descriptor_invalid"))?;
    if entry.adapter_metadata.get("helper_obligation") != Some(&expected) {
        return Err(internal("helper_source_descriptor_mismatch"));
    }
    let record = actual
        .helper_record
        .as_ref()
        .ok_or_else(|| internal("helper_source_record_missing"))?;
    record.validate_binding()?;
    Ok(record.clone())
}
