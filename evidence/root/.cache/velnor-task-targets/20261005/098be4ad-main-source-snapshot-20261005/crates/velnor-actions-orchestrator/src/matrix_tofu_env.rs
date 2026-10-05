//! Tofu obligation-step env derivation: per-root data and cache dirs.
//!
//! Split from `matrix_step` (size gate). Both constructors read the
//! obligation task ID the identity map carries and derive the
//! normalized root from its key segment; non-tofu and task-less
//! extras map to `None`.

use std::collections::BTreeMap;

use velnor_actions_contract::Stack;

use crate::OrchestratorError;
use crate::task_report::TASK_ID_ENV;

/// Isolated per-root `TF_DATA_DIR` for tofu obligation extras, if any.
///
/// Reads the obligation task ID the identity map carries, derives the
/// normalized root from its key segment, and builds the Velnor-owned
/// data dir under the runner-temp base. Non-tofu and task-less extras
/// (fetch steps) map to `None`; the reserved-key rule in
/// `task_step_env` already rejected any caller-supplied `TF_*`, so
/// this constructor is the sole source of the rendered key.
///
/// # Errors
///
/// Returns a contract error when the data-dir derivation fails.
pub(super) fn tofu_data_dir_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(root) = tofu_root_for_extra(extra) else {
        return Ok(None);
    };
    velnor_actions_tofu::tofu_data_dir_under(
        velnor_actions_mise::runtime_paths::TOFU_DATA_BASE_EXPR,
        &root,
    )
    .map(Some)
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

/// Job-private per-root `TF_PLUGIN_CACHE_DIR` for tofu extras, if any.
///
/// Same task-ID derivation as the data dir, under the provider-cache
/// base: init reads restored providers from here. The slug matches
/// the data dir's; only the base differs, so the two stay separate
/// by construction.
///
/// # Errors
///
/// Returns a contract error when the cache-dir derivation fails.
pub(super) fn tofu_plugin_cache_dir_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(root) = tofu_root_for_extra(extra) else {
        return Ok(None);
    };
    velnor_actions_tofu::tofu_cache_dir_under(
        crate::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR,
        &root,
    )
    .map(Some)
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

/// Normalized tofu root for obligation extras, when one applies.
///
/// `None` for non-tofu and task-less extras (fetch steps) plus
/// unparsable key segments; the reserved-key rule already rejected
/// any caller-supplied `TF_*`, so these constructors are the sole
/// source of the rendered keys.
fn tofu_root_for_extra(extra: &BTreeMap<String, String>) -> Option<String> {
    let task_id = extra.get(TASK_ID_ENV)?;
    if super::obligation_stack(task_id) != Some(Stack::Tofu) {
        return None;
    }
    let key = crate::extension_schemas::task_key_segment(task_id)?;
    Some(velnor_actions_tofu::root_for_key(&key))
}
