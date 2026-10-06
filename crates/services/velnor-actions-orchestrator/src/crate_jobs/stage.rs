//! Crate-job stack predicates plus tofu root-job lane staging.
//!
//! The predicates dispatch one task's driver/runner selection by
//! stack (tofu binds no rust tools through an explicit arm, never the
//! rust unknown-spelling fallthrough). Staging bounds independent
//! root concurrency: root jobs fan out one per validation root, and
//! without staging they all run concurrently. Staging chains lanes
//! through `needs`: over sorted tofu job IDs, job `i` waits for job
//! `i - max`, so each lane (ID index modulo `max_parallel_jobs`)
//! serializes while lanes run free — at most `max_parallel_jobs` root
//! jobs run at once. Rust jobs never stage. Edges point backward in
//! sorted order, so the graph stays acyclic by construction.

use std::collections::BTreeMap;

use velnor_actions_contract::Stack;
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::Job;
use velnor_actions_rust::tool_needs;

/// Tool needs backing one task's driver/runner selection.
///
/// Tofu tasks bind no rust tools through an explicit arm, never the
/// rust unknown-spelling fallthrough.
pub(crate) fn needs(task: &ProposedTask) -> velnor_actions_rust::ToolNeeds {
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        return velnor_actions_rust::ToolNeeds {
            mbx: false,
            nextest: false,
        };
    }
    tool_needs(&task.identity.compile_driver, &task.identity.test_runner)
}

/// True when the task needs the Rust toolchain (tofu tasks never do;
/// unknown stacks keep the rust fallthrough, matching [`needs`]).
pub(crate) fn is_rust(task: &ProposedTask) -> bool {
    !is_opentofu(task)
}

/// True when the task compiles through MBX (unknown spellings are Cargo).
pub(crate) fn is_mbx(task: &ProposedTask) -> bool {
    needs(task).mbx
}

/// True when the task runs tests through Nextest.
pub(crate) fn is_nextest(task: &ProposedTask) -> bool {
    needs(task).nextest
}

/// True when the task runs through the pinned Opentofu driver.
pub(crate) fn is_opentofu(task: &ProposedTask) -> bool {
    Stack::from_id(&task.stack_id) == Some(Stack::Tofu)
}

/// Run condition for lane-staged tofu jobs: collect independent lane
/// failures instead of skipping past them (`needs` still serializes;
/// conclusions still fold; step-level coverage gates unaffected).
/// Only jobs that gain a lane predecessor carry it: first-in-lane
/// jobs keep the default plan-gated condition.
pub(crate) const STAGED_TOFU_JOB_CONDITION: &str = "always()";

/// Append lane-staging `needs` to tofu root jobs.
///
/// `tofu_ids` names the staged jobs; every other job keeps its
/// `needs` untouched. Staging is deterministic for a fixed ID set.
/// Jobs that gain a lane predecessor also gain [`STAGED_TOFU_JOB_CONDITION`].
pub(crate) fn stage_tofu_root_jobs(
    jobs: &mut [(String, Job)],
    tofu_ids: &[String],
    max_parallel_jobs: u32,
) {
    let mut lanes: Vec<&str> = tofu_ids.iter().map(String::as_str).collect();
    lanes.sort_unstable();
    lanes.dedup();
    let width = usize::try_from(max_parallel_jobs.max(1)).unwrap_or(1);
    let mut predecessors: BTreeMap<&str, &str> = BTreeMap::new();
    for (index, id) in lanes.iter().enumerate() {
        if index >= width {
            predecessors.insert(id, lanes[index - width]);
        }
    }
    if predecessors.is_empty() {
        return;
    }
    for (id, job) in jobs.iter_mut() {
        if let Some(previous) = predecessors.get(id.as_str()) {
            job.needs.push((*previous).to_owned());
            job.condition = Some(STAGED_TOFU_JOB_CONDITION.to_owned());
        }
    }
}
