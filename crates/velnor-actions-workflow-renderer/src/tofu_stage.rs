// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::{Job, TOFU_JOB_ID_PREFIX};

const STAGED_TOFU_JOB_CONDITION: &str = "always()";

pub(crate) fn stage_tofu_jobs(jobs: &mut BTreeMap<String, Job>, max_parallel_jobs: u32) {
    let width = usize::try_from(max_parallel_jobs.max(1)).unwrap_or(1);
    let consumers = jobs.keys().filter(|id| id.starts_with(TOFU_JOB_ID_PREFIX));
    let producers = jobs
        .iter()
        .filter(|(_, job)| job.producer_kind() == Some(ProducerKind::TofuProviders))
        .map(|(id, _)| id);
    let lane: Vec<String> = consumers.chain(producers).cloned().collect();
    for (index, id) in lane.iter().enumerate().skip(width) {
        let Some(job) = jobs.get_mut(id) else {
            continue;
        };
        job.needs.push(lane[index - width].clone());
        job.needs.sort();
        job.needs.dedup();
        if job.producer_kind().is_none() {
            job.condition = Some(match job.condition.as_deref() {
                Some(prior) => format!("{STAGED_TOFU_JOB_CONDITION} && ({prior})"),
                None => STAGED_TOFU_JOB_CONDITION.to_owned(),
            });
        }
    }
}
