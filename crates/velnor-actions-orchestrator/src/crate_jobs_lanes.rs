// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{CrateJob, Job, Step, StepKind};

pub(super) struct ProducerLanes {
    pub(super) root: String,
    pub(super) nextest: bool,
    pub(super) lanes: Vec<Step>,
}

pub(super) fn lane_records(job: &Job, model: &CrateJob) -> Vec<Step> {
    job.steps
        .iter()
        .filter(|step| {
            let StepKind::TaskExecution { task_id, .. } = &step.kind else {
                return false;
            };
            model
                .obligations
                .iter()
                .find(|obligation| obligation.task_id == *task_id)
                .is_some_and(|obligation| is_producer_lane(&obligation.kind))
        })
        .cloned()
        .collect()
}

fn is_producer_lane(kind: &str) -> bool {
    velnor_actions_rust::runs_before_parallel_checks(kind)
        || velnor_actions_rust::concurrent_target_lane(kind).is_some()
}
