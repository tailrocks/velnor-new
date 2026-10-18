//! Crate-job identity: grouping plus owning-job binding.
//!
//! Runnable obligations group by `(package_id, configuration)`; the
//! contract assigns one stable unbranded job ID per group. Both the
//! renderer ([`build_crate_jobs`](crate::crate_jobs::build_crate_jobs))
//! and the planner resolve through this module, so the plan's
//! per-entry job binding and the rendered jobs can never disagree.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, ProposedTask, Stack, TOFU_JOB_ID_PREFIX, WorkflowEvent,
    assign_crate_job_ids, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

/// Owning job ID for one plan-universe member.
///
/// Package-less workspace tasks belong to the plan job; every other
/// member resolves through the same runnable grouping and ID
/// assignment as the renderer, so the plan's job binding and the
/// rendered jobs can never disagree. `None` only when the member
/// is absent from the assignment inputs (never from the plan path).
pub(crate) fn job_id_for_member(
    tasks: &[ProposedTask],
    member: &ProposedTask,
    event: WorkflowEvent,
) -> Option<String> {
    if Stack::from_id(&member.stack_id) == Some(Stack::Mise) {
        return Some(format!("check-{}", member.identity.unit_id));
    }
    if member.identity.unit_id.is_empty() {
        return Some(PLAN_JOB_ID.to_owned());
    }
    if event == WorkflowEvent::Qualification {
        return qualification_job_id_for_member(member).ok();
    }
    let grouped = group_runnable(tasks);
    let assigned = assign_group_ids(&grouped);
    assigned
        .get(&(
            member.identity.unit_id.clone(),
            member.configuration.clone(),
        ))
        .cloned()
}

/// Stable one-entry job ID derived from the plan matrix key.
///
/// These IDs exist beside grouped crate IDs so Qualification can run
/// each cache lane in its own job without changing ordinary CI grouping.
pub(crate) fn qualification_job_id_for_member(
    member: &ProposedTask,
) -> Result<String, velnor_actions_contract::ContractError> {
    let id = matrix_id_for_task_group(&member.stack_id, &member.task_id)?;
    let key = matrix_key_for_id(&id)?;
    qualification_job_id_for_key(&member.stack_id, &key)
}

/// Stable one-entry job ID for one validated stack and matrix key.
pub(crate) fn qualification_job_id_for_key(
    stack_id: &str,
    matrix_key: &str,
) -> Result<String, velnor_actions_contract::ContractError> {
    let prefix = match Stack::from_id(stack_id) {
        Some(Stack::Rust) => CRATE_JOB_ID_PREFIX,
        Some(Stack::Tofu) => TOFU_JOB_ID_PREFIX,
        _ => {
            return Err(velnor_actions_contract::ContractError::identity(
                "qualification.job_id",
                "unsupported_stack",
            ));
        }
    };
    let suffix = matrix_key.strip_prefix("m-").ok_or_else(|| {
        velnor_actions_contract::ContractError::identity(
            "qualification.job_id",
            "malformed_matrix_key",
        )
    })?;
    velnor_actions_contract::validate_matrix_key(matrix_key)?;
    Ok(format!("{prefix}qualification-{suffix}"))
}

/// Stable IDs for every runnable group, prefixed per stack.
///
/// All-tofu groups take `tofu-<slug>` (their exact-set identity);
/// every other group keeps `rust-<slug>`, so mixed groups inherit
/// the rust union exactly like their install set. Namespaces never
/// collide across prefixes; within a prefix the contract
/// disambiguates colliding slugs. Both the renderer and the planner
/// resolve through here, so bindings can never disagree.
pub(crate) fn assign_group_ids(
    grouped: &BTreeMap<(String, String), Vec<&ProposedTask>>,
) -> BTreeMap<(String, String), String> {
    let (rust, tofu) = id_inputs(grouped);
    let mut assigned = assign_crate_job_ids(&rust, CRATE_JOB_ID_PREFIX);
    assigned.extend(assign_crate_job_ids(&tofu, TOFU_JOB_ID_PREFIX));
    assigned
}

/// True when every group member is a tofu task.
///
/// The single partition behind both the `tofu-` ID namespace and the
/// `OpenToFu — <root>` display: ID and display derive from the same
/// predicate over the same members, so they can never disagree.
pub(crate) fn group_is_tofu(members: &[&ProposedTask]) -> bool {
    !members.is_empty()
        && members
            .iter()
            .all(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

/// Runnable tasks by `(package_id, configuration)` in sorted order.
///
/// Skips tasks without test targets (no command is emitted for them)
/// and package-less workspace tasks (the plan job owns that scope).
pub(crate) fn group_runnable(
    tasks: &[ProposedTask],
) -> BTreeMap<(String, String), Vec<&ProposedTask>> {
    let mut grouped: BTreeMap<(String, String), Vec<&ProposedTask>> = BTreeMap::new();
    for task in tasks {
        if !crate::crate_jobs::is_runnable(task) {
            continue;
        }
        grouped
            .entry((task.identity.unit_id.clone(), task.configuration.clone()))
            .or_default()
            .push(task);
    }
    grouped
}

/// ID-assignment input triples: `(package_id, package_name, configuration)`.
type IdInputs = BTreeSet<(String, String, String)>;

/// ID-assignment inputs per stack: one triple per group, named by its
/// first member.
fn id_inputs(grouped: &BTreeMap<(String, String), Vec<&ProposedTask>>) -> (IdInputs, IdInputs) {
    let mut rust = BTreeSet::new();
    let mut tofu = BTreeSet::new();
    for ((package_id, configuration), members) in grouped {
        let Some(first) = members.first() else {
            continue;
        };
        let triple = (
            package_id.clone(),
            first.display_name.clone(),
            configuration.clone(),
        );
        if group_is_tofu(members) {
            tofu.insert(triple);
        } else {
            rust.insert(triple);
        }
    }
    (rust, tofu)
}
