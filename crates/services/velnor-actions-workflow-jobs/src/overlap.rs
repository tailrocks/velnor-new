//! Overlap-then-join for independent prep (parallelism §10 fixture).
//!
//! V1 emits no `parallel:`/`background:` step syntax (the pinned
//! actionlint cannot parse it), so independent preparation overlaps at
//! the job level: two branch jobs with no ordering edge run concurrently
//! and the consumer joins both via `needs`. This module wires the join
//! and proves branch independence instead of trusting the caller.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::Job;

use velnor_actions_workflow_steps::RenderError;

/// Typed overlap: two independent prep branches plus their join job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepOverlap {
    /// Branch job ID: verified artifact download.
    pub download_job: String,
    /// Branch job ID: pinned image preparation.
    pub image_job: String,
    /// Consumer job ID joining both branches before use.
    pub join_job: String,
}

/// Wire the join: validate independence, then need both branches.
///
/// All three jobs must exist and be distinct; neither branch may reach
/// the other (directly or transitively) or depend on the join. Missing
/// branch IDs are appended to the join's `needs` in branch order.
/// # Errors
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

/// True when `target` is reachable from `from` through `needs` edges.
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
