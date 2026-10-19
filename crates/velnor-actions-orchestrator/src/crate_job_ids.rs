//! Shared planner/renderer execution ownership for fixed Rust capacity phases.
//!
//! Runnable obligations group by package, unchanged configuration and fixed
//! execution responsibility. The contract assigns collision-safe unbranded
//! package/configuration IDs before Rust phase suffixes are appended. Both
//! [`build_crate_jobs`](crate::crate_jobs::build_crate_jobs) and the planner
//! resolve through this module, so emitted owners and per-entry job bindings
//! cannot disagree about the complete runnable task universe.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, ContractError, ProposedTask, Stack, TOFU_JOB_ID_PREFIX,
    assign_crate_job_ids,
};
use velnor_actions_rust::RustJobPhase;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

/// One package/configuration and its closed execution responsibility.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CrateGroupKey {
    pub(crate) package_id: String,
    pub(crate) configuration: String,
    pub(crate) phase: Option<RustJobPhase>,
}

impl CrateGroupKey {
    fn for_member(member: &ProposedTask) -> Result<Self, ContractError> {
        let phase = match Stack::require_known(&member.stack_id)? {
            Stack::Rust => Some(RustJobPhase::from_task_kind(&member.task_kind)?),
            Stack::Tofu | Stack::Mise => None,
        };
        Ok(Self {
            package_id: member.identity.unit_id.clone(),
            configuration: member.configuration.clone(),
            phase,
        })
    }
}

/// Immutable ownership assignment for one complete proposal universe.
///
/// The planner and renderer construct this once per batch. Gate lookup also
/// indexes runnable proposals once; no entry or edge rebuilds the universe.
pub(crate) struct JobOwners<'a> {
    assigned: BTreeMap<CrateGroupKey, String>,
    runnable: BTreeMap<&'a str, &'a ProposedTask>,
}
impl<'a> JobOwners<'a> {
    /// Group and assign the complete immutable inventory once.
    pub(crate) fn new(tasks: &'a [ProposedTask]) -> Result<Self, ContractError> {
        let grouped = group_runnable(tasks)?;
        Ok(Self::from_grouped(tasks, &grouped))
    }
    /// Reuse the renderer's already validated complete grouping.
    pub(crate) fn from_grouped(
        tasks: &'a [ProposedTask],
        grouped: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>,
    ) -> Self {
        Self {
            assigned: assign_group_ids(grouped),
            runnable: tasks
                .iter()
                .filter(|task| crate::crate_jobs::is_runnable(task))
                .map(|task| (task.task_id.as_str(), task))
                .collect(),
        }
    }
    /// Exact assigned ID for one emitted group.
    pub(crate) fn group_id(&self, key: &CrateGroupKey) -> Option<&String> {
        self.assigned.get(key)
    }
    /// Runnable gate definitions only; no-target and absent gates remain vacuous.
    pub(crate) fn runnable_member(&self, id: &str) -> Option<&ProposedTask> {
        self.runnable.get(id).copied()
    }
    /// Owning job from the same complete assignment used by emission.
    ///
    /// No-target tasks have no execution owner. Workspace tasks belong to Plan;
    /// named checks retain their dedicated check job and do not enter Rust phases.
    /// Every other member resolves through the renderer's runnable grouping and
    /// complete collision assignment. `None` means no execution is emitted or the
    /// member is absent from those assignment inputs; a runnable plan member must
    /// have an owner. This lookup never manufactures an unsuffixed Rust fallback.
    ///
    /// # Errors
    /// Rejects an unknown member stack or Rust kind. Construction already
    /// rejects invalid kinds anywhere in the complete runnable universe.
    pub(crate) fn for_member(
        &self,
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
        Ok(self
            .assigned
            .get(&CrateGroupKey::for_member(member)?)
            .cloned())
    }
}

/// Assign collision-safe package/configuration IDs once, then fixed phases.
///
/// Configuration identity is never overloaded with a phase. All-Tofu jobs keep
/// their existing namespace and IDs; Rust groups have no legacy unsuffixed job.
/// Collocated Rust and Tofu obligations occupy different execution groups.
/// Prefixes separate their namespaces; the contract disambiguates colliding
/// package/configuration slugs within each namespace before any phase suffix
/// is applied. Planner and renderer use this same complete assignment rather
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
            let base = if group_is_tofu(members) {
                tofu_ids.get(&base_key)
            } else {
                rust_ids.get(&base_key)
            }?;
            let id = key.phase.map_or_else(
                || base.clone(),
                |phase| format!("{base}-{}", phase.as_str()),
            );
            Some((key.clone(), id))
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

/// Runnable proposals, each in exactly one package/configuration/phase group.
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

/// Existing package/configuration collision input triples, independent of phase:
/// `(package_id, package_name, configuration)`.
type IdInputs = BTreeSet<(String, String, String)>;

/// Collect one package/configuration identity per stack from each group's first
/// member. Deduplication across Rust phases makes base collision assignment
/// independent of which fixed responsibilities a package happens to contain.
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
