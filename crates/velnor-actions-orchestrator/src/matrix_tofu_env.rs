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

/// Full isolated Tofu environment, authored after rejecting caller extras.
pub(super) fn isolation_env_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    tofu_root_for_extra(extra)?.map_or_else(
        || Ok(BTreeMap::new()),
        |root| crate::tofu_config_step::task_env(&root),
    )
}

/// Normalized tofu root for obligation extras, when one applies.
///
/// `None` for non-tofu and task-less extras (fetch steps) plus
/// invalid key segments fail closed; the reserved-key rule already rejected
/// any caller-supplied `TF_*`, so these constructors are the sole
/// source of the rendered keys.
fn tofu_root_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(task_id) = extra.get(TASK_ID_ENV) else {
        return Ok(None);
    };
    if super::obligation_stack(task_id) != Some(Stack::Tofu) {
        return Ok(None);
    }
    let key = crate::extension_schemas::task_key_segment(task_id)
        .ok_or_else(|| crate::internal::internal("tofu_unparsable_key"))?;
    Ok(Some(velnor_actions_tofu::root_for_key(&key)?))
}
