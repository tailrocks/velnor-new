//! Stack task-ID tail and shard-suffix validators.

use crate::errors::ContractError;

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
    if let Some(last) = tail.last()
        && let Some(shard) = last.strip_prefix("shard-")
    {
        validate_shard_suffix(shard)?;
        tail = &tail[..tail.len() - 1];
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

/// Validate a `shard-<index>-of-<count>` suffix body.
fn validate_shard_suffix(shard: &str) -> Result<(), ContractError> {
    let Some((index, count)) = shard.split_once("-of-") else {
        return Err(ContractError::identity("task_id", "bad_shard_suffix"));
    };
    let (Ok(index), Ok(count)) = (index.parse::<u32>(), count.parse::<u32>()) else {
        return Err(ContractError::identity("task_id", "bad_shard_suffix"));
    };
    validate_shard(index, count).map_err(|_| ContractError::identity("task_id", "bad_shard_range"))
}
