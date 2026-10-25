use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, ContractError, ProposedTask, Stack, TOFU_JOB_ID_PREFIX,
    assign_crate_job_ids,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

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

pub(crate) struct JobOwners<'a> {
    assigned: BTreeMap<CrateGroupKey, String>,
    runnable: BTreeMap<&'a str, &'a ProposedTask>,
}
impl<'a> JobOwners<'a> {
    pub(crate) fn new(tasks: &'a [ProposedTask]) -> Result<Self, ContractError> {
        let grouped = group_runnable(tasks)?;
        Ok(Self::from_grouped(tasks, &grouped))
    }
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
    pub(crate) fn group_id(&self, key: &CrateGroupKey) -> Option<&String> {
        self.assigned.get(key)
    }
    pub(crate) fn runnable_member(&self, id: &str) -> Option<&ProposedTask> {
        self.runnable.get(id).copied()
    }
    pub(crate) fn for_member(
        &self,
        member: &ProposedTask,
    ) -> Result<Option<String>, ContractError> {
        if member.no_targets {
            return Ok(None);
        }
        if matches!(
            Stack::require_known(&member.stack_id)?,
            Stack::Mise | Stack::Native
        ) {
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

pub(crate) fn group_is_tofu(members: &[&ProposedTask]) -> bool {
    !members.is_empty()
        && members
            .iter()
            .all(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

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

type IdInputs = BTreeSet<(String, String, String)>;

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
