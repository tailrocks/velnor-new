//! Build preservation ownership from validated task and control inventories.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::{
    CONTROL_WORKFLOW_PATHS, ControlEvidenceRecord, WorkloadSourceTransfer,
    WorkloadTransferDisposition,
};
use velnor_actions_contract::VelnorConfig;
use velnor_actions_workflow_renderer::RenderedTree;

use super::{OwnershipTransfer, assemble};
use crate::{OrchestratorError, prepare::GenerationPreparation};

const CONFIG_PATH: &str = ".velnor/config.toml";

/// Assemble generated outputs only after the full source map revalidates.
pub(super) fn assemble_for_preparation(
    source: &Path,
    destination: &Path,
    tree: &RenderedTree,
    prep: &GenerationPreparation,
) -> Result<(), OrchestratorError> {
    let config = &prep.config;
    config
        .validate(CONFIG_PATH)
        .map_err(contract_error)?;
    let active_tasks: BTreeSet<String> = prep
        .discovery
        .proposals
        .iter()
        .map(|proposal| proposal.task_id.clone())
        .collect();
    let active_controls: BTreeSet<String> = config
        .controls
        .entries
        .iter()
        .map(|control| control.id().to_owned())
        .collect();
    let source_plan = config
        .workloads
        .transfer_sources(CONFIG_PATH, &active_tasks, &active_controls)
        .map_err(contract_error)?;
    let control_sources = config.controls.transfer_sources();
    let owned_paths = config.controls.owned_paths();
    let transfers = verified_transfers(&source_plan, &control_sources, &owned_paths)?;
    assemble(
        source,
        destination,
        tree,
        &transfers,
        &owned_paths,
        CONTROL_WORKFLOW_PATHS,
    )
}

fn verified_transfers<'a>(
    source_plan: &'a WorkloadSourceTransfer,
    control_sources: &[ControlEvidenceRecord],
    owned_paths: &[&str],
) -> Result<Vec<OwnershipTransfer<'a>>, OrchestratorError> {
    validate_control_sources(source_plan, control_sources, owned_paths)?;
    let mut transfers = Vec::new();
    for file in &source_plan.files {
        let workflow = file.path.starts_with(".github/workflows/");
        if !workflow && file.disposition != WorkloadTransferDisposition::Preserve {
            return Err(contract_problem("nonworkflow_source_not_preserved"));
        }
        if file.disposition != WorkloadTransferDisposition::Regenerate {
            continue;
        }
        if !workflow || !matches!(file.mode.as_str(), "100644" | "100755") || file.jobs.is_empty() {
            return Err(contract_problem("unsafe_source_transfer_record"));
        }
        transfers.push(OwnershipTransfer {
            path: &file.path,
            sha256: &file.sha256,
        });
    }
    Ok(transfers)
}

fn validate_control_sources(
    source_plan: &WorkloadSourceTransfer,
    control_sources: &[ControlEvidenceRecord],
    owned_paths: &[&str],
) -> Result<(), OrchestratorError> {
    for path in owned_paths {
        if !CONTROL_WORKFLOW_PATHS.contains(path)
            || !control_sources.iter().any(|record| record.path == *path)
        {
            return Err(contract_problem("active_control_path_missing_evidence"));
        }
    }
    for record in control_sources {
        if !owned_paths.contains(&record.path)
            || !CONTROL_WORKFLOW_PATHS.contains(&record.path)
        {
            return Err(contract_problem("inactive_control_transfer_source"));
        }
        let Some(file) = source_plan.files.iter().find(|file| file.path == record.path) else {
            return Err(contract_problem("control_source_missing_from_retained_tree"));
        };
        if file.disposition != WorkloadTransferDisposition::Regenerate
            || file.sha256 != record.sha256
            || file.git_blob_sha1 != record.git_blob_sha1.unwrap_or_default()
            || record.source_revision != Some(source_plan.source_set.source_revision.as_str())
        {
            return Err(contract_problem("control_source_not_fully_mapped"));
        }
    }
    Ok(())
}

fn contract_error(error: impl ToString) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

fn contract_problem(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
