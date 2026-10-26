// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

pub(crate) fn jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            typed_runner(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn typed_runner(runs_on: &str) -> bool {
    matches!(
        RunsOn::parse(runs_on),
        Ok(RunsOn::Hosted(_) | RunsOn::ScaleSet(_))
    )
}
