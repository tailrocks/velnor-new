//! Validation for fixed qualification-cache operation inputs.

use std::collections::BTreeMap;

use crate::RenderError;

pub(super) fn validate_qualification_cache_env(
    operation: &str,
    step_env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    for (key, value) in step_env {
        if key == "GH_TOKEN" {
            if operation != "observe-qualification-cache-backend-v1"
                || value != "${{ github.token }}"
            {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_token_scope_invalid".to_owned(),
                ));
            }
        } else if key != "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE"
            && !key.starts_with("VELNOR_QUALIFICATION_CACHE_")
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "qualification_cache_env_not_supported:{key}"
            )));
        }
    }
    Ok(())
}
