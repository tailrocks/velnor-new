use super::files::parse_layer;
use crate::OrchestratorError;

/// Return the owner-and-layer-specific runtime identity filename.
///
/// # Errors
/// Returns an error when the owner ID or layer is outside its closed contract.
pub fn qualification_cache_runtime_identity_filename(
    owner_id: &str,
    layer: &str,
) -> Result<String, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    let layer = parse_layer(layer)?;
    Ok(format!(
        "runtime-identity-{owner_id}-{}.json",
        layer.as_str()
    ))
}

/// Return the filename for a post-import identity record.
///
/// # Errors
/// Returns an error when the owner ID or layer is outside its closed contract.
pub fn qualification_cache_runtime_after_import_filename(
    owner_id: &str,
    layer: &str,
) -> Result<String, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    let layer = parse_layer(layer)?;
    Ok(format!(
        "runtime-identity-{owner_id}-{}-after-import.json",
        layer.as_str()
    ))
}
