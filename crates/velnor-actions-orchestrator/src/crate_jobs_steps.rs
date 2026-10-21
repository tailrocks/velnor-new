//! Sequential test-build barrier and independent parallel check target lanes.

use super::{CrateJobInputs, build_jobs::cap_cargo_jobs};
use crate::{OrchestratorError, internal::internal};
use std::collections::BTreeSet;
use velnor_actions_contract::{CrateJob, CrateObligation, Step};

/// Report-wrapped obligation steps in validated order.
///
/// In a Rust job every obligation except the sequential producers (the
/// Nextest build) is a member of one parallel group, when at least two
/// exist. No member may gate on another member. Clippy and documentation
/// members compile in their own target lane below the workspace target.
pub(super) fn obligation_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_opentofu: bool,
    use_mbx: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let sequential = |obligation: &CrateObligation| {
        velnor_actions_rust::runs_before_parallel_checks(&obligation.kind)
    };
    let concurrent = !use_opentofu
        && model
            .obligations
            .iter()
            .filter(|obligation| !sequential(obligation))
            .count()
            >= 2;
    let members: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter(|obligation| concurrent && !sequential(obligation))
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    // The MBX bundle producer is the last sequential compile (the test build):
    // the marker after it lets the export run even when a later check fails.
    let bundle_producer = use_mbx
        .then(|| model.obligations.iter().rposition(sequential))
        .flatten();
    let target_root =
        crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
    let seed = seed_step(input, model, concurrent, target_root)?;
    let mut steps = Vec::with_capacity(model.obligations.len() + usize::from(seed.is_some()));
    let mut seed = seed;
    for (index, obligation) in model.obligations.iter().enumerate() {
        let parallel = members.contains(obligation.task_id.as_str());
        if !parallel && steps.iter().any(|step: &Step| step.parallel) {
            return Err(internal("crate_parallel_group_split"));
        }
        if parallel
            && obligation
                .gated_by
                .iter()
                .any(|gate| members.contains(gate.as_str()))
        {
            return Err(internal("crate_parallel_member_gate"));
        }
        if parallel && let Some(seed) = seed.take() {
            steps.push(seed);
        }
        let lane = parallel
            .then(|| velnor_actions_rust::concurrent_target_lane(&obligation.kind))
            .flatten()
            .map(|lane| lane_target_dir(target_root, lane));
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(input.max_parallel_jobs);
        let mut step = crate::matrix_step::obligation_step(
            obligation,
            input.catalog,
            &downstream,
            cap,
            input.helper_version,
            lane.as_deref(),
        )?;
        step.parallel = parallel;
        if let Some(options) = input.crates.get(&model.package_name) {
            cap_cargo_jobs(&mut step, options.build_jobs);
        }
        steps.push(step);
        if bundle_producer == Some(index) {
            steps.push(velnor_actions_workflow_renderer::steps::mbx_producer_mark_step()?);
        }
    }
    Ok(steps)
}

/// Workspace-relative Cargo target lane directory for one concurrent check.
fn lane_target_dir(workspace_root: &str, lane: &str) -> String {
    if workspace_root.is_empty() {
        format!("target/{lane}")
    } else {
        format!("{workspace_root}/target/{lane}")
    }
}

/// Seed only real concurrent Cargo lanes after a declared build producer.
fn seed_step(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    concurrent: bool,
    workspace_root: &str,
) -> Result<Option<Step>, OrchestratorError> {
    if !input.seed_parallel_targets
        || !concurrent
        || !model
            .obligations
            .iter()
            .any(|task| velnor_actions_rust::runs_before_parallel_checks(&task.kind))
    {
        return Ok(None);
    }
    let lanes: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter_map(|task| velnor_actions_rust::concurrent_target_lane(&task.kind))
        .collect();
    if lanes.is_empty() {
        return Ok(None);
    }
    let target_root = lane_target_dir(workspace_root, "");
    Ok(Some(
        velnor_actions_workflow_renderer::steps::seed_parallel_targets_step(
            target_root.trim_end_matches('/'),
            &lanes.into_iter().collect::<Vec<_>>(),
        )?,
    ))
}
