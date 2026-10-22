//! Affected selection for Bun member jobs (hard decision 3).
//!
//! Bun jobs own no Rust obligations. A member is planned from what it reads:
//! its directory, the files its workspace shares, its `workspace:` dependents
//! and the inputs `[[stacks.bun.inputs]]` declares. Doubt selects the member:
//! an unknown change set, a global-config path, or reads outside its
//! directory that no complete declaration covers. Full-run events never skip
//! a member.

use std::collections::BTreeSet;

use velnor_actions_bun::{
    affected_members, forced_members, job_id, member_job_scripts, suite_job_id,
};
use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};

use super::ChangeEvidence;
use crate::discover::Discovery;

/// Plan dispositions for every Bun member and suite job, in member-name order.
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
        // Generation already proved the configuration; a member job omitted
        // because its suites claim every script has no disposition.
        if !matches!(member_job_scripts(member, bun), Ok(None)) {
            jobs.push(PlanJob {
                job_id: job_id(member),
                disposition,
            });
        }
        // Every suite job, gating or not, carries its member's disposition,
        // so a `not_affected` member's suites are skipped and pass.
        for suite in bun
            .members
            .get(&member.name)
            .into_iter()
            .flat_map(|options| options.suites.keys())
        {
            jobs.push(PlanJob {
                job_id: suite_job_id(member, suite),
                disposition,
            });
        }
    }
    jobs
}
