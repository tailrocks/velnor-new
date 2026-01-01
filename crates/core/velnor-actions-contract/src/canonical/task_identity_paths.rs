//! Canonical path fields used by task identity validation and hashing.
use super::{TaskIdentity, normalize_posix_path};
use crate::errors::ContractError;

pub(super) fn normalized(identity: &TaskIdentity) -> Result<TaskIdentity, ContractError> {
    let mut normalized = identity.clone();
    normalized.project_root = normalize_posix_path(&identity.project_root)?;
    normalized.working_dir = normalize_posix_path(&identity.working_dir)?;
    normalized.component_id = normalize_posix_path(&identity.component_id)?;
    for input in &mut normalized.inputs {
        input.path = normalize_posix_path(&input.path)?;
    }
    Ok(normalized)
}
