use std::collections::BTreeMap;

use crate::RenderError;

/// Validate a fixed env map: `A-Z0-9_` keys, single-line clean values.
///
/// Expressions stay allowlisted, never blanket-banned: only fixed
/// runner-provided spans pass (see the private `expressions` module).
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    validate_env_in_scope(env, false, false)
}

/// Validate one fixed qualification-cache operation environment.
pub(crate) fn validate_qualification_cache_env(
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    validate_env_in_scope(env, false, true)
}

pub(crate) fn validate_composite_qualification_cache_env(
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    validate_env_in_scope(env, true, true)
}

/// Validate environment for a generated composite action's typed inputs.
pub(crate) fn validate_composite_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    validate_env_in_scope(env, true, false)
}

fn validate_env_in_scope(
    env: &BTreeMap<String, String>,
    composite: bool,
    qualification_cache: bool,
) -> Result<(), RenderError> {
    for (key, value) in env {
        if key.is_empty()
            || (key != "TF_VAR_github_tokens"
                && !key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
        {
            return Err(RenderError::BadCommand(format!("bad_env_key:{key}")));
        }
        if value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
        {
            return Err(RenderError::BadCommand(format!("bad_env_value:{key}")));
        }
        if key == "GH_TOKEN" && qualification_cache {
            if value != "${{ github.token }}" {
                return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
            }
        } else if qualification_cache && composite {
            crate::expressions::check_composite_qualification_cache_env_value(key, value)?;
        } else if composite {
            crate::expressions::check_composite_env_value(key, value)?;
        } else if qualification_cache {
            crate::expressions::check_qualification_cache_env_value(key, value)?;
        } else {
            crate::expressions::check_env_value(key, value)?;
        }
        crate::steps::scan_for_private_subcommands(key)?;
        crate::steps::scan_for_private_subcommands(value)?;
    }
    Ok(())
}
