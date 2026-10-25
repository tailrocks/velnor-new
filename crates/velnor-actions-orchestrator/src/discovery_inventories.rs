use crate::OrchestratorError;
use std::path::Path;
use velnor_actions_contract::{FileIndex, VelnorConfig};

pub(super) type TaskInventories = (
    crate::workflow_task_jobs::bun_inventory::BunInventory,
    crate::workflow_task_jobs::dbt_inventory::DbtInventory,
    crate::workflow_task_jobs::ansible_inventory::AnsibleInventory,
);

fn bun_inventory(
    root: &Path,
    index: &FileIndex,
    config: &VelnorConfig,
) -> Result<crate::workflow_task_jobs::bun_inventory::BunInventory, OrchestratorError> {
    match &config.stacks.bun {
        Some(stack) => {
            crate::workflow_task_jobs::bun_inventory::BunInventory::load(root, index, stack)
        }
        None => Ok(crate::workflow_task_jobs::bun_inventory::BunInventory::default()),
    }
}

pub(super) fn task_inventories(
    root: &Path,
    index: &FileIndex,
    config: &VelnorConfig,
) -> Result<TaskInventories, OrchestratorError> {
    use crate::workflow_task_jobs::{
        ansible_inventory::AnsibleInventory, dbt_inventory::DbtInventory,
    };
    Ok((
        bun_inventory(root, index, config)?,
        DbtInventory::for_config(root, index, config)?,
        AnsibleInventory::for_config(root, index, config)?,
    ))
}
