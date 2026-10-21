//! Affected selection for Ansible root jobs (hard decision 3).
//!
//! Ansible jobs own no Rust obligations. A root job is planned from its
//! directory, the hash-pinned requirements file and the inventory (the files
//! every root reads). Doubt selects the job: an unknown change set, a
//! global-config path (`ChangeEvidence::paths` is `None`) and a full-run event
//! never skip a root.

use std::collections::BTreeSet;

use velnor_actions_ansible::affected_roots;
use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};

use super::ChangeEvidence;
use crate::discover::Discovery;

/// Plan dispositions for every Ansible root job, in root order.
pub(crate) fn dispositions(
    config: &VelnorConfig,
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> Vec<PlanJob> {
    if config.stacks.ansible.is_none() {
        return Vec::new();
    }
    let inventory = &discovery.ansible;
    let dirs: Vec<String> = inventory
        .roots
        .iter()
        .map(|root| root.dir.clone())
        .collect();
    let selected = match (selects_affected_work(event), affected, &evidence.paths) {
        (true, Some(_), Some(changed)) => Some(affected_roots(&dirs, &inventory.shared, changed)),
        _ => None,
    };
    inventory
        .roots
        .iter()
        .map(|root| PlanJob {
            job_id: root.job_id(),
            disposition: if selected
                .as_ref()
                .is_none_or(|selected| selected.contains(&root.dir))
            {
                JobDisposition::Run
            } else {
                JobDisposition::NotAffected
            },
        })
        .collect()
}
