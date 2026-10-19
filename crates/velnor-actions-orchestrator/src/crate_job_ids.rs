//! Shared planner/renderer execution ownership for crate jobs.
//!
//! Rust obligations use one job per package. `ToFu` obligations use one job per
//! package and configuration. Both the renderer and planner use this module.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, ContractError, ProposedTask, Stack, TOFU_JOB_ID_PREFIX, WorkflowEvent,
    assign_crate_job_ids, assign_rust_crate_job_ids, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_rust::TaskKind;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

/// One package/configuration and its closed execution responsibility.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CrateGroupKey {
    pub(crate) stack_id: String,
    pub(crate) package_id: String,
    pub(crate) configuration: String,
}

impl CrateGroupKey {
    fn for_member(member: &ProposedTask) -> Result<Self, ContractError> {
        let stack = Stack::require_known(&member.stack_id)?;
        if stack == Stack::Rust {
            TaskKind::parse(&member.task_kind)?;
        }
        Ok(Self {
            stack_id: member.stack_id.clone(),
            package_id: member.identity.unit_id.clone(),
            configuration: if stack == Stack::Tofu {
                member.configuration.clone()
            } else {
                String::new()
            },
        })
    }
}

/// Owning job from the same complete assignment used by emission.
///
/// No-target tasks have no execution owner. Workspace tasks belong to Plan;
/// named checks retain their dedicated check job and do not enter crate jobs.
/// Every other member resolves through the renderer's runnable grouping and
/// complete collision assignment. `None` means no execution is emitted or the
/// member is absent from those assignment inputs; a runnable plan member must
/// have an owner. This lookup never manufactures an unsuffixed Rust fallback.
///
/// # Errors
/// Rejects unknown stacks or Rust task kinds in the runnable universe.
pub(crate) fn job_id_for_member(
    tasks: &[ProposedTask],
    member: &ProposedTask,
    event: WorkflowEvent,
) -> Result<Option<String>, ContractError> {
    if member.no_targets {
        return Ok(None);
    }
    if Stack::require_known(&member.stack_id)? == Stack::Mise {
        return Ok(Some(format!("check-{}", member.identity.unit_id)));
    }
    if event == WorkflowEvent::Qualification
        && Stack::from_id(&member.stack_id) == Some(Stack::Tofu)
    {
        return Ok(Some(qualification_job_id_for_member(member)?));
    }
    if member.identity.unit_id.is_empty() {
        return Ok(Some(PLAN_JOB_ID.to_owned()));
    }
    let grouped = group_runnable(tasks)?;
    let assigned = assign_group_ids(&grouped);
    Ok(assigned.get(&CrateGroupKey::for_member(member)?).cloned())
}

/// Stable job ID for one `ToFu` Qualification entry.
pub(crate) fn qualification_job_id_for_member(
    member: &ProposedTask,
) -> Result<String, ContractError> {
    if Stack::require_known(&member.stack_id)? != Stack::Tofu {
        return Err(ContractError::identity(
            "qualification.job_id",
            "unsupported_stack",
        ));
    }
    let id = matrix_id_for_task_group(&member.stack_id, &member.task_id)?;
    qualification_job_id_for_key(&member.stack_id, &matrix_key_for_id(&id)?)
}

/// Connect each Qualification job to the job that owns its source task.
pub(crate) fn qualification_job_sources(
    tasks: &[ProposedTask],
) -> Result<BTreeMap<String, String>, ContractError> {
    let grouped = group_runnable(tasks)?;
    let assigned = assign_group_ids(&grouped);
    let mut sources = BTreeMap::new();
    for task in tasks
        .iter()
        .filter(|task| crate::crate_jobs::is_runnable(task))
    {
        if Stack::require_known(&task.stack_id)? == Stack::Mise {
            continue;
        }
        let source = assigned
            .get(&CrateGroupKey::for_member(task)?)
            .ok_or_else(|| {
                ContractError::identity("qualification.sources", "grouped_job_missing")
            })?;
        let destination = if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
            qualification_job_id_for_member(task)?
        } else {
            source.clone()
        };
        if let Some(previous) = sources.insert(destination, source.clone())
            && previous != *source
        {
            return Err(ContractError::Collision(
                "qualification source job".to_owned(),
            ));
        }
    }
    Ok(sources)
}

/// Keep source bindings only for qualification jobs present in the workflow.
pub(crate) fn qualification_job_sources_for_ir(
    tasks: &[ProposedTask],
    ir: &velnor_actions_contract::WorkflowIr,
) -> Result<BTreeMap<String, String>, ContractError> {
    let sources = qualification_job_sources(tasks)?;
    for id in ir
        .jobs
        .keys()
        .filter(|id| id.starts_with(CRATE_JOB_ID_PREFIX) || id.starts_with("tofu-qualification-"))
    {
        if !sources.contains_key(id) {
            return Err(ContractError::identity(
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

fn qualification_job_id_for_key(stack_id: &str, matrix_key: &str) -> Result<String, ContractError> {
    if Stack::require_known(stack_id)? != Stack::Tofu {
        return Err(ContractError::identity(
            "qualification.job_id",
            "unsupported_stack",
        ));
    }
    let suffix = matrix_key
        .strip_prefix("m-")
        .ok_or_else(|| ContractError::identity("qualification.job_id", "malformed_matrix_key"))?;
    velnor_actions_contract::validate_matrix_key(matrix_key)?;
    Ok(format!("{TOFU_JOB_ID_PREFIX}qualification-{suffix}"))
}

/// Give one stable ID to each Rust package for all its configurations.
///
/// Rust jobs use package identity. `ToFu` jobs use package and configuration.
/// The prefixes separate their namespaces.
pub(crate) fn assign_group_ids(
    grouped: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>,
) -> BTreeMap<CrateGroupKey, String> {
    let (rust, tofu) = id_inputs(grouped);
    let rust_ids = assign_rust_crate_job_ids(
        &rust
            .iter()
            .map(|(package_id, package_name, _)| (package_id.clone(), package_name.clone()))
            .collect(),
        CRATE_JOB_ID_PREFIX,
    );
    let tofu_ids = assign_crate_job_ids(&tofu, TOFU_JOB_ID_PREFIX);
    grouped
        .keys()
        .filter_map(|key| {
            let base = if Stack::from_id(&key.stack_id) == Some(Stack::Tofu) {
                tofu_ids.get(&(key.package_id.clone(), key.configuration.clone()))
            } else {
                rust_ids.get(&key.package_id)
            }?;
            Some((key.clone(), base.clone()))
        })
        .collect()
}

/// The same stack predicate owns both the Tofu namespace and display label.
pub(crate) fn group_is_tofu(members: &[&ProposedTask]) -> bool {
    !members.is_empty()
        && members
            .iter()
            .all(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

/// Runnable proposals in package groups.
///
/// Tasks without executable targets remain absent because no command is
/// emitted for them. Package-less workspace tasks stay in Plan. Rust task
/// configurations share a package group. `ToFu` keeps separate configurations.
///
/// # Errors
/// Rejects unknown stacks or Rust kinds rather than dropping an obligation.
pub(crate) fn group_runnable(
    tasks: &[ProposedTask],
) -> Result<BTreeMap<CrateGroupKey, Vec<&ProposedTask>>, ContractError> {
    let mut grouped: BTreeMap<CrateGroupKey, Vec<&ProposedTask>> = BTreeMap::new();
    for task in tasks {
        if crate::crate_jobs::is_runnable(task) {
            grouped
                .entry(CrateGroupKey::for_member(task)?)
                .or_default()
                .push(task);
        }
    }
    Ok(grouped)
}

/// Package identity inputs used for collision checks:
/// `(package_id, package_name, configuration)`.
type IdInputs = BTreeSet<(String, String, String)>;

/// Collect one package identity per Rust group and one package/configuration
/// identity per `ToFu` group.
fn id_inputs(
    grouped: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>,
) -> (BTreeSet<(String, String, String)>, IdInputs) {
    let mut rust = BTreeSet::new();
    let mut tofu = BTreeSet::new();
    for key in grouped.keys() {
        let members = &grouped[key];
        let Some(first) = members.first() else {
            continue;
        };
        let triple = (
            key.package_id.clone(),
            first.display_name.clone(),
            key.configuration.clone(),
        );
        if Stack::from_id(&key.stack_id) == Some(Stack::Tofu) {
            tofu.insert(triple);
        } else {
            rust.insert(triple);
        }
    }
    (rust, tofu)
}

#[cfg(test)]
#[path = "crate_job_ids_tests.rs"]
mod tests;
