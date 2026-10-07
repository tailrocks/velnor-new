//! Verified check outcomes and their staged persistence.
//!
//! The execute phase fills one outcome per run; the writers persist
//! the execution receipt and any scenario evidence bytes under the
//! entry's staged home, refusing pre-existing files.

use std::path::Path;

use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract_config::config::MAX_CHECK_EXECUTION_RECEIPT_BYTES;
use velnor_actions_contract_workflow::{MatrixEntry, Plan};
use velnor_actions_mise::DiscoveredCheck;
use velnor_actions_orchestrator_check_evidence::scenario::EvidenceReceipt;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::{internal, internal_contract};

/// Verified outcome of one qualified check run.
#[derive(Debug)]
pub struct CheckOutcome {
    /// Verified scenario evidence, when the check declares any.
    pub evidence: Option<EvidenceReceipt>,
    /// Observed container receipt, when the run was containerized.
    pub container:
        Option<velnor_actions_orchestrator_check_preparation::container_receipts::ContainerReceipt>,
    /// Proofs for native tools already installed on the runner.
    pub system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    /// Receipts for the explicitly qualified installation closure.
    pub qualified_tools:
        Vec<velnor_actions_orchestrator_check_acquisition::tools::QualifiedToolReceipt>,
}

/// Persist the execution receipt for one finished check run.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] when the receipt exceeds
/// the admitted byte bound and [`OrchestratorError::Io`] for
/// unwritable paths; pre-existing files error, never overwrite.
pub fn write_execution_receipt(
    temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    item: &DiscoveredCheck,
    outcome: &CheckOutcome,
) -> Result<(), OrchestratorError> {
    let mut execution = velnor_actions_orchestrator_check_evidence::gate::execution_receipt(
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
    velnor_actions_orchestrator_core::exclusive_write::create_dir_no_symlink(temp, &base)?;
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive(
        &base.join("check-execution.json"),
        &bytes,
        "check_execution",
    )
}

/// Persist verified scenario evidence bytes under the entry home.
///
/// # Errors
///
/// Returns [`OrchestratorError::Io`] for unwritable paths;
/// pre-existing files error, never overwrite.
pub fn save_evidence(
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
    velnor_actions_orchestrator_core::exclusive_write::create_dir_no_symlink(temp, parent)?;
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive(
        &artifact,
        &receipt.bytes,
        "check_evidence",
    )?;
    Ok(())
}
