// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_CONCURRENCY_CANCEL, STATE_SEAL_CONCURRENCY_GROUP, STATE_SEAL_WORKFLOW_NAME,
};
use velnor_actions_contract::{Concurrency, Job, Trigger, WorkflowIr};

use crate::RenderError;
use crate::lane_share::LaneShare;
use crate::render::PLAN_JOB_ID;

pub(crate) struct StateSealWorkflow {
    pub(crate) ir: WorkflowIr,
    pub(crate) shared: LaneShare,
    pub(crate) jobs: BTreeMap<String, Job>,
}

pub(crate) fn split_state_seal(
    ir: &WorkflowIr,
    shared: &mut LaneShare,
    jobs: &mut BTreeMap<String, Job>,
) -> Result<Option<StateSealWorkflow>, RenderError> {
    let producers: BTreeSet<String> = shared
        .jobs
        .iter()
        .filter(|(_, job)| {
            job.producer_kind()
                .is_some_and(|kind| kind != ProducerKind::TofuProviders)
        })
        .map(|(id, _)| id.clone())
        .collect();
    if producers.is_empty() {
        return Ok(None);
    }
    let plan = jobs
        .get(PLAN_JOB_ID)
        .cloned()
        .ok_or_else(|| invalid("plan_missing"))?;
    let mut sealed = shared.split_off(&producers);
    sealed.jobs.insert(PLAN_JOB_ID.to_owned(), plan.clone());
    let mut sealed_jobs = BTreeMap::from([(PLAN_JOB_ID.to_owned(), plan)]);
    for id in &producers {
        let job = jobs
            .remove(id)
            .ok_or_else(|| invalid("producer_missing_before_share"))?;
        sealed_jobs.insert(id.clone(), job);
    }
    let sealed_ir = WorkflowIr {
        name: STATE_SEAL_WORKFLOW_NAME.to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: ir.triggers.push_branches.clone(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: ir.permissions.clone(),
        concurrency: Concurrency {
            group: STATE_SEAL_CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: STATE_SEAL_CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: sealed_jobs.clone(),
    };
    check_needs_closed(&sealed_ir.jobs)?;
    check_needs_closed(&shared.jobs)?;
    Ok(Some(StateSealWorkflow {
        ir: sealed_ir,
        shared: sealed,
        jobs: sealed_jobs,
    }))
}

fn check_needs_closed(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        if let Some(missing) = job.needs.iter().find(|need| !jobs.contains_key(*need)) {
            return Err(invalid(&format!("needs_cross_workflow:{id}:{missing}")));
        }
    }
    Ok(())
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("state_seal_workflow:{problem}"))
}
