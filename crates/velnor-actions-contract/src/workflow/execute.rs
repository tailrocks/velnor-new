//! Executable obligation references for one matrix entry.
use crate::errors::ContractError;
use crate::ids::validate_task_id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Executable obligations: single IDs or shard arrays (stack-neutral keys).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecuteTaskIds {
    /// Named task references.
    pub tasks: BTreeMap<String, ExecuteTaskRef>,
}
/// One named task reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExecuteTaskRef {
    /// Single task ID.
    Single(String),
    /// Sharded task IDs.
    Shards(Vec<String>),
}
impl ExecuteTaskIds {
    /// Validate reference names and every task ID they name.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        for (name, task_ref) in &self.tasks {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(ContractError::identity("execute_task_ids", "bad_name"));
            }
            task_ref.validate()?;
        }
        Ok(())
    }
}
impl ExecuteTaskRef {
    /// Validate the named task ID or shard IDs.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::Single(id) => validate_task_id(id),
            Self::Shards(ids) => {
                if ids.is_empty() {
                    return Err(ContractError::identity("execute_task_ids", "empty_shards"));
                }
                for id in ids {
                    validate_task_id(id)?;
                }
                Ok(())
            }
        }
    }
}
