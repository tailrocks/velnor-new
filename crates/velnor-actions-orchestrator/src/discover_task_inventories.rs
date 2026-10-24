//! Build inventories for configured Bun, dbt, and Ansible stacks.

use std::path::Path;

use velnor_actions_contract::{FileIndex, VelnorConfig};

use crate::OrchestratorError;
use crate::workflow_task_jobs::{
    ansible_inventory::AnsibleInventory, bun_inventory::BunInventory, dbt_inventory::DbtInventory,
};

pub(super) type TaskInventories = (BunInventory, DbtInventory, AnsibleInventory);

/// Load all stack inventories from the same file index.
pub(super) fn load(
    root: &Path,
    index: &FileIndex,
    config: &VelnorConfig,
) -> Result<TaskInventories, OrchestratorError> {
    Ok((
        bun_inventory(root, index, config)?,
        DbtInventory::for_config(root, index, config)?,
        AnsibleInventory::for_config(root, index, config)?,
    ))
}

/// Bun member inventory for the configured `[stacks.bun]` (empty without it).
fn bun_inventory(
    root: &Path,
    index: &FileIndex,
    config: &VelnorConfig,
) -> Result<BunInventory, OrchestratorError> {
    match &config.stacks.bun {
        Some(stack) => BunInventory::load(root, index, stack),
        None => Ok(BunInventory::default()),
    }
}
