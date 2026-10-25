use std::collections::BTreeSet;

use velnor_actions_bun::{
    affected_members, forced_members, job_id, member_job_scripts, suite_job_id,
};
use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};

use velnor_actions_workflow_renderer::verification_jobs::BUN_ASSETS_SUFFIX;

use super::ChangeEvidence;
use crate::discover::Discovery;

pub(crate) fn dispositions(
    config: &VelnorConfig,
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> Vec<PlanJob> {
    let Some(bun) = &config.stacks.bun else {
        return Vec::new();
    };
    let inventory = &discovery.bun;
    let workspaces = inventory.workspaces();
    let selected = match (selects_affected_work(event), affected, &evidence.paths) {
        (true, Some(_), Some(changed)) => {
            let mut selected = affected_members(&workspaces, changed, &bun.inputs);
            selected.extend(forced_members(&inventory.reads, &bun.inputs));
            Some(selected)
        }
        _ => None,
    };
    let has_release_assets = config
        .private_sources
        .as_ref()
        .is_some_and(|sources| !sources.release_assets.is_empty());
    let mut jobs = Vec::new();
    for member in workspaces.iter().flat_map(|workspace| &workspace.members) {
        let disposition = if selected
            .as_ref()
            .is_none_or(|selected| selected.contains(&member.name))
        {
            JobDisposition::Run
        } else {
            JobDisposition::NotAffected
        };
        if !matches!(member_job_scripts(member, bun), Ok(None)) {
            jobs.push(PlanJob {
                job_id: job_id(member),
                disposition,
            });
        }
        let suites: Vec<&String> = bun
            .members
            .get(&member.name)
            .into_iter()
            .flat_map(|options| options.suites.keys())
            .collect();
        for suite in &suites {
            jobs.push(PlanJob {
                job_id: suite_job_id(member, suite),
                disposition,
            });
        }
        if !suites.is_empty() && has_release_assets {
            jobs.push(PlanJob {
                job_id: format!("{}{BUN_ASSETS_SUFFIX}", job_id(member)),
                disposition,
            });
        }
    }
    jobs
}
