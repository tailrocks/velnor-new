//! Optional preparation for independent Rust check targets.

use std::collections::BTreeSet;

use crate::OrchestratorError;
use velnor_actions_contract::{CrateJob, Step};

use super::CrateJobInputs;

/// Return a bounded seed step when this job has a build and parallel check lanes.
pub(super) fn parallel_target_seed_step(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_rust: bool,
    use_opentofu: bool,
) -> Result<Option<Step>, OrchestratorError> {
    let parallel_count = model
        .obligations
        .iter()
        .filter(|obligation| !velnor_actions_rust::runs_before_parallel_checks(&obligation.kind))
        .count();
    if !input.seed_parallel_targets
        || !use_rust
        || use_opentofu
        || parallel_count < 2
        || !model
            .obligations
            .iter()
            .any(|obligation| velnor_actions_rust::runs_before_parallel_checks(&obligation.kind))
    {
        return Ok(None);
    }
    let lanes: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter_map(|obligation| velnor_actions_rust::concurrent_target_lane(&obligation.kind))
        .collect();
    if lanes.is_empty() {
        return Ok(None);
    }
    let root = crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
    let target = if root.is_empty() {
        "target".to_owned()
    } else {
        format!("{root}/target")
    };
    Ok(Some(
        velnor_actions_workflow_renderer::steps::seed_parallel_targets_step(
            &target,
            &lanes.into_iter().collect::<Vec<_>>(),
        )?,
    ))
}
