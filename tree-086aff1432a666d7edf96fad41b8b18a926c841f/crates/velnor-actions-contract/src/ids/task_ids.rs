//! Stack task-ID tail and shard validators.

use crate::errors::ContractError;

use super::shard::split_shard_suffix;
use super::{validate_component, validate_path_segments};

/// Validate shard index/count (`shard-<index>-of-<count>` inputs).
pub(super) fn validate_shard(index: u32, count: u32) -> Result<(), ContractError> {
    if count >= 1 && index >= 1 && index <= count {
        Ok(())
    } else {
        Err(ContractError::identity("shard", "bad_shard_range"))
    }
}

/// Validate the tail of a `stack/` task ID.
pub(super) fn validate_stack_task_id(rest: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 4 {
        return Err(ContractError::identity("task_id", "too_few_segments"));
    }
    validate_component(parts[0], "stack_id")?;
    let mut tail = parts.as_slice();
    if split_shard_suffix(rest).is_some() {
        tail = &tail[..tail.len() - 1];
    } else if tail.last().is_some_and(|last| last.starts_with("shard-")) {
        return Err(ContractError::identity("task_id", "bad_shard_suffix"));
    }
    if tail.len() < 4 {
        return Err(ContractError::identity("task_id", "too_few_segments"));
    }
    let config = tail[tail.len() - 1];
    let kind = tail[tail.len() - 2];
    validate_component(kind, "task_kind")?;
    validate_component(config, "configuration")?;
    validate_path_segments(&tail[1..tail.len() - 2].join("/"), "manifest_key")?;
    Ok(())
}
