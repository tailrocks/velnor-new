use std::collections::BTreeMap;

use velnor_actions_contract::{Job, ProposedTask, Stack};
use velnor_actions_rust::tool_needs;

pub(crate) fn needs(task: &ProposedTask) -> velnor_actions_rust::ToolNeeds {
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        return velnor_actions_rust::ToolNeeds {
            mbx: false,
            nextest: false,
        };
    }
    tool_needs(&task.identity.compile_driver, &task.identity.test_runner)
}

pub(crate) fn is_rust(task: &ProposedTask) -> bool {
    !is_opentofu(task)
}

pub(crate) fn is_mbx(task: &ProposedTask) -> bool {
    needs(task).mbx
}

pub(crate) fn is_nextest(task: &ProposedTask) -> bool {
    needs(task).nextest
}

pub(crate) fn is_opentofu(task: &ProposedTask) -> bool {
    Stack::from_id(&task.stack_id) == Some(Stack::Tofu)
}

pub(crate) const STAGED_TOFU_JOB_CONDITION: &str = "always()";

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
