//! Crate-job identity: grouping plus owning-job binding.
//!
//! Rust tasks use package IDs. `ToFu` tasks use package and configuration IDs.
//! The contract assigns one stable job ID to each group. The renderer
//! ([`build_crate_jobs`](crate::crate_jobs::build_crate_jobs)) and planner use
//! this module. This keeps plan bindings equal to rendered jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::jobs::assign_rust_crate_job_ids;
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
    if event == WorkflowEvent::Qualification
        && Stack::from_id(&member.stack_id) == Some(Stack::Tofu)
    {
        return qualification_job_id_for_member(member).ok();
    }
    if Stack::from_id(&member.stack_id) == Some(Stack::Mise) {
        return Some(format!("check-{}", member.identity.unit_id));
    }
    if member.identity.unit_id.is_empty() {
        return Some(PLAN_JOB_ID.to_owned());
    }
    let grouped = group_runnable(tasks);
    let assigned = assign_group_ids(&grouped);
    assigned.get(&group_key(member)).cloned()
}

/// Stable job ID for `ToFu` Qualification or a named check.
pub(crate) fn qualification_job_id_for_member(
    member: &ProposedTask,
) -> Result<String, velnor_actions_contract::ContractError> {
    match Stack::from_id(&member.stack_id) {
        Some(Stack::Mise) => return Ok(format!("check-{}", member.identity.unit_id)),
        Some(Stack::Tofu) => {}
        _ => {
            return Err(velnor_actions_contract::ContractError::identity(
                "qualification.job_id",
                "unsupported_stack",
            ));
        }
    }
    let id = matrix_id_for_task_group(&member.stack_id, &member.task_id)?;
    let key = matrix_key_for_id(&id)?;
    qualification_job_id_for_key(&member.stack_id, &key)
}

/// Connect each Rust or `ToFu` Qualification job to its source job.
/// This connection keeps the source job's runner selector.
/// # Errors
pub(crate) fn qualification_job_sources(
    tasks: &[ProposedTask],
) -> Result<BTreeMap<String, String>, velnor_actions_contract::ContractError> {
    let assigned = assign_group_ids(&group_runnable(tasks));
    let mut sources = BTreeMap::new();
    for task in tasks {
        if !crate::crate_jobs::is_runnable(task)
            || Stack::from_id(&task.stack_id) == Some(Stack::Mise)
        {
            continue;
        }
        let group = group_key(task);
        let source_id = assigned.get(&group).ok_or_else(|| {
            velnor_actions_contract::ContractError::identity(
                "qualification.sources",
                "grouped_job_missing",
            )
        })?;
        let qualification_id = if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
            qualification_job_id_for_member(task)?
        } else {
            source_id.clone()
        };
        if let Some(previous) = sources.insert(qualification_id, source_id.clone())
            && previous != *source_id
        {
            return Err(velnor_actions_contract::ContractError::Collision(
                "qualification source job".to_owned(),
            ));
        }
    }
    Ok(sources)
}

/// Keep source bindings only for jobs present in this workflow IR.
/// # Errors
pub(crate) fn qualification_job_sources_for_ir(
    tasks: &[ProposedTask],
    ir: &velnor_actions_contract::WorkflowIr,
) -> Result<BTreeMap<String, String>, velnor_actions_contract::ContractError> {
    let sources = qualification_job_sources(tasks)?;
    for id in ir.jobs.keys().filter(|id| is_qualification_crate_job(id)) {
        if !sources.contains_key(id) {
            return Err(velnor_actions_contract::ContractError::identity(
                "qualification.sources",
                "source_job_missing",
            ));
        }
    }
    Ok(sources
        .into_iter()
        .filter(|(id, _)| ir.jobs.contains_key(id))
        .collect())
}

fn is_qualification_crate_job(id: &str) -> bool {
    id.starts_with(CRATE_JOB_ID_PREFIX) || id.starts_with("tofu-qualification-")
}

/// Stable one-entry job ID for one validated stack and matrix key.
pub(crate) fn qualification_job_id_for_key(
    stack_id: &str,
    matrix_key: &str,
) -> Result<String, velnor_actions_contract::ContractError> {
    let prefix = match Stack::from_id(stack_id) {
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
    let rust_ids = assign_rust_crate_job_ids(&rust, CRATE_JOB_ID_PREFIX);
    let mut assigned: BTreeMap<(String, String), String> = rust_ids
        .into_iter()
        .map(|(package_id, job_id)| ((package_id, String::new()), job_id))
        .collect();
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

/// Runnable tasks by package, with `ToFu` kept separate by configuration.
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
        grouped.entry(group_key(task)).or_default().push(task);
    }
    grouped
}

/// Package-only groups use an empty configuration key; `ToFu` keeps its config key.
fn group_key(task: &ProposedTask) -> (String, String) {
    let configuration = if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        task.configuration.clone()
    } else {
        String::new()
    };
    (task.identity.unit_id.clone(), configuration)
}

/// Rust package identity and `ToFu` package/configuration identity.
type IdInputs = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// ID-assignment inputs per stack: one identity per group.
fn id_inputs(grouped: &BTreeMap<(String, String), Vec<&ProposedTask>>) -> IdInputs {
    let mut rust = BTreeSet::new();
    let mut tofu = BTreeSet::new();
    for ((package_id, configuration), members) in grouped {
        let Some(first) = members.first() else {
            continue;
        };
        if group_is_tofu(members) {
            tofu.insert((
                package_id.clone(),
                first.display_name.clone(),
                configuration.clone(),
            ));
        } else {
            rust.insert((package_id.clone(), first.display_name.clone()));
        }
    }
    (rust, tofu)
}
