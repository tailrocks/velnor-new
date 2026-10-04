//! Bounded provider storage locators; exact root authority stays in receipts.
use crate::OrchestratorError;

const CANDIDATE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-provider-candidate";

pub(super) fn candidate_path(root: &str) -> Result<String, OrchestratorError> {
    Ok(format!("{CANDIDATE_BASE_EXPR}/{}", root_locator(root)?))
}

pub(super) fn tofu_cache_path(root: &str) -> Result<String, OrchestratorError> {
    crate::tofu_cache::tofu_provider_cache_path(root)
}

pub(super) fn root_locator(root: &str) -> Result<String, OrchestratorError> {
    velnor_actions_tofu::validate_normalized_root(root)?;
    velnor_actions_tofu::tofu_root_locator(root).map_err(OrchestratorError::from)
}
