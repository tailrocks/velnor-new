// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_bun::{
    affected_members, forced_members, job_id, member_job_scripts, suite_job_id,
};
use velnor_actions_contract::{
    JobDisposition, PlanJob, VelnorConfig, WorkflowEvent, selects_affected_work,
};
use velnor_actions_workflow_renderer::verification_jobs::BUN_ASSETS_SUFFIX;

use super::ChangeEvidence;
use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::internal;

pub(crate) fn dispositions(
    config: &VelnorConfig,
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
    bun_job_lanes: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<PlanJob>, OrchestratorError> {
    let Some(bun) = &config.stacks.bun else {
        return Ok(Vec::new());
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
    project_lane_jobs(jobs, bun_job_lanes)
}

fn project_lane_jobs(
    logical_jobs: Vec<PlanJob>,
    bun_job_lanes: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<PlanJob>, OrchestratorError> {
    let mut physical_jobs = Vec::new();
    for logical_job in logical_jobs {
        let outputs = bun_job_lanes
            .get(&logical_job.job_id)
            .filter(|outputs| !outputs.is_empty())
            .ok_or_else(|| internal("bun_job_lane_missing"))?;
        physical_jobs.extend(outputs.iter().map(|job_id| PlanJob {
            job_id: job_id.clone(),
            disposition: logical_job.disposition,
        }));
    }
    Ok(physical_jobs)
}
