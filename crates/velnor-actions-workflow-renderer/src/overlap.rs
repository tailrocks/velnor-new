// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

use crate::RenderError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepOverlap {
    pub download_job: String,
    pub image_job: String,
    pub join_job: String,
}

pub fn wire_prep_join(
    jobs: &mut BTreeMap<String, Job>,
    spec: &PrepOverlap,
) -> Result<(), RenderError> {
    let ids = [&spec.download_job, &spec.image_job, &spec.join_job];
    if ids[0] == ids[1] || ids[0] == ids[2] || ids[1] == ids[2] {
        return Err(RenderError::InvalidWorkflow(
            "prep_overlap_not_distinct".to_owned(),
        ));
    }
    for id in &ids {
        if !jobs.contains_key(id.as_str()) {
            return Err(RenderError::InvalidWorkflow(format!(
                "prep_overlap_unknown_job:{id}"
            )));
        }
    }
    if reaches(jobs, &spec.download_job, &spec.image_job)
        || reaches(jobs, &spec.image_job, &spec.download_job)
    {
        return Err(RenderError::InvalidWorkflow(
            "prep_overlap_dependent".to_owned(),
        ));
    }
    if reaches(jobs, &spec.download_job, &spec.join_job)
        || reaches(jobs, &spec.image_job, &spec.join_job)
    {
        return Err(RenderError::InvalidWorkflow(
            "prep_overlap_cycle".to_owned(),
        ));
    }
    let Some(join) = jobs.get_mut(spec.join_job.as_str()) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "prep_overlap_unknown_job:{}",
            spec.join_job
        )));
    };
    for branch in [&spec.download_job, &spec.image_job] {
        if !join.needs.contains(branch) {
            join.needs.push(branch.clone());
        }
    }
    Ok(())
}

fn reaches(jobs: &BTreeMap<String, Job>, from: &str, target: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from.to_owned()];
    while let Some(next) = stack.pop() {
        if next == target {
            return true;
        }
        if !seen.insert(next.clone()) {
            continue;
        }
        if let Some(job) = jobs.get(next.as_str()) {
            stack.extend(job.needs.iter().cloned());
        }
    }
    false
}
