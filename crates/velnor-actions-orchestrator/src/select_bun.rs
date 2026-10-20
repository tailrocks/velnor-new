//! Affected selection for Bun member jobs (hard decision 3).
//!
//! Bun jobs own no Rust obligations. A member is planned from what it reads:
//! its directory, the files its workspace shares, its `workspace:` dependents
//! and the inputs `[[stacks.bun.inputs]]` declares. Doubt selects the member:
//! an unknown change set, a global-config path, or reads outside its
//! directory that no complete declaration covers. Full-run events never skip
//! a member.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_bun::{affected_members, forced_members, job_id};
use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};

use super::ChangeEvidence;
use crate::OrchestratorError;
use crate::workflow_task_jobs::bun_inventory::BunInventory;

/// Plan dispositions for every Bun member job, in member-name order.
pub(crate) fn dispositions(
    root: &Path,
    config: &VelnorConfig,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> Result<Vec<PlanJob>, OrchestratorError> {
    let Some(bun) = &config.stacks.bun else {
        return Ok(Vec::new());
    };
    let (index, _) = crate::discover_index::build_file_index(root, &config.discovery.exclude)?;
    let inventory = BunInventory::load(root, &index, bun)?;
    let workspaces = inventory.workspaces();
    let selected = match (selects_affected_work(event), affected, &evidence.paths) {
        (true, Some(_), Some(changed)) => {
            let mut selected = affected_members(&workspaces, changed, &bun.inputs);
            selected.extend(forced_members(&inventory.reads, &bun.inputs));
            Some(selected)
        }
        _ => None,
    };
    Ok(workspaces
        .iter()
        .flat_map(|workspace| &workspace.members)
        .map(|member| PlanJob {
            job_id: job_id(member),
            disposition: if selected
                .as_ref()
                .is_none_or(|selected| selected.contains(&member.name))
            {
                JobDisposition::Run
            } else {
                JobDisposition::NotAffected
            },
        })
        .collect())
}
