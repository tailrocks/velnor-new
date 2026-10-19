//! Shared planner/renderer execution ownership: one job per crate.
//!
//! Runnable obligations group by stack, package and unchanged configuration.
//! Every Rust package/configuration owns exactly one job whose checks run as
//! parallel steps (see `docs/hard-decisions.md`); there are no per-phase
//! jobs. The contract assigns collision-safe package/configuration IDs. Both
//! [`build_crate_jobs`](crate::crate_jobs::build_crate_jobs) and the planner
//! resolve through this module, so emitted owners and per-entry job bindings
//! cannot disagree about the complete runnable task universe.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, ContractError, ProposedTask, Stack, TOFU_JOB_ID_PREFIX,
    assign_crate_job_ids,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

/// One stack's package/configuration execution owner.
///
/// The stack keeps collocated Rust and `OpenTofu` obligations in separate jobs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CrateGroupKey {
    pub(crate) package_id: String,
    pub(crate) configuration: String,
    pub(crate) stack: Stack,
}

impl CrateGroupKey {
    fn for_member(member: &ProposedTask) -> Result<Self, ContractError> {
        Ok(Self {
            package_id: member.identity.unit_id.clone(),
            configuration: member.configuration.clone(),
            stack: Stack::require_known(&member.stack_id)?,
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
/// have an owner.
///
/// # Errors
/// Rejects unknown stacks or Rust task kinds in the runnable universe.
pub(crate) fn job_id_for_member(
    tasks: &[ProposedTask],
    member: &ProposedTask,
) -> Result<Option<String>, ContractError> {
    if member.no_targets {
        return Ok(None);
    }
    if Stack::require_known(&member.stack_id)? == Stack::Mise {
        return Ok(Some(format!("check-{}", member.identity.unit_id)));
    }
    if member.identity.unit_id.is_empty() {
        return Ok(Some(PLAN_JOB_ID.to_owned()));
    }
    let grouped = group_runnable(tasks)?;
    let assigned = assign_group_ids(&grouped);
    Ok(assigned.get(&CrateGroupKey::for_member(member)?).cloned())
}

/// Assign collision-safe package/configuration IDs once.
///
/// All-Tofu jobs keep their namespace and IDs; each Rust package/configuration
/// owns one `rust-<slug>` job. Collocated Rust and Tofu obligations occupy
/// different execution groups. Prefixes separate their namespaces; the
/// contract disambiguates colliding package/configuration slugs within each
/// namespace. Planner and renderer use this same complete assignment rather
/// than assigning independently from a selected subset of obligations.
pub(crate) fn assign_group_ids(
    grouped: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>,
) -> BTreeMap<CrateGroupKey, String> {
    let (rust, tofu) = id_inputs(grouped);
    let rust_ids = assign_crate_job_ids(&rust, CRATE_JOB_ID_PREFIX);
    let tofu_ids = assign_crate_job_ids(&tofu, TOFU_JOB_ID_PREFIX);
    grouped
        .iter()
        .filter_map(|(key, members)| {
            let base_key = (key.package_id.clone(), key.configuration.clone());
            let id = if group_is_tofu(members) {
                tofu_ids.get(&base_key)
            } else {
                rust_ids.get(&base_key)
            }?;
            Some((key.clone(), id.clone()))
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

/// Runnable proposals, each in exactly one stack/package/configuration group.
///
/// Tasks without executable targets remain absent because no command is
/// emitted for them. Package-less workspace tasks stay in Plan, and named
/// checks retain dedicated owners outside this grouping. The ordered map
/// preserves deterministic emission without changing configuration identity.
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

/// Package/configuration collision input triples:
/// `(package_id, package_name, configuration)`.
type IdInputs = BTreeSet<(String, String, String)>;

/// Collect one package/configuration identity per stack from each group's
/// first member.
fn id_inputs(grouped: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>) -> (IdInputs, IdInputs) {
    let mut rust = BTreeSet::new();
    let mut tofu = BTreeSet::new();
    for (key, members) in grouped {
        let Some(first) = members.first() else {
            continue;
        };
        let triple = (
            key.package_id.clone(),
            first.display_name.clone(),
            key.configuration.clone(),
        );
        if group_is_tofu(members) {
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
