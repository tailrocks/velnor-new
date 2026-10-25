use std::collections::BTreeSet;

use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};
use velnor_actions_dbt::{affected_projects, job_id};

use super::ChangeEvidence;
use crate::discover::Discovery;

pub(crate) fn dispositions(
    config: &VelnorConfig,
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> Vec<PlanJob> {
    if config.stacks.dbt.is_none() {
        return Vec::new();
    }
    let dirs = discovery.dbt.dirs();
    let selected = match (selects_affected_work(event), affected, &evidence.paths) {
        (true, Some(_), Some(changed)) => Some(affected_projects(&dirs, changed)),
        _ => None,
    };
    discovery
        .dbt
        .projects
        .iter()
        .map(|project| PlanJob {
            job_id: job_id(project),
            disposition: if selected
                .as_ref()
                .is_none_or(|selected| selected.contains(&project.dir))
            {
                JobDisposition::Run
            } else {
                JobDisposition::NotAffected
            },
        })
        .collect()
}
