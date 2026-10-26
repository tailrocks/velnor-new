// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::Stack;

use crate::OrchestratorError;
use crate::task_report::TASK_ID_ENV;

pub(super) fn tofu_data_dir_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(root) = tofu_root_for_extra(extra)? else {
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

pub(super) fn tofu_plugin_cache_dir_for_extra(
    extra: &BTreeMap<String, String>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(root) = tofu_root_for_extra(extra)? else {
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
    velnor_actions_tofu::root_for_key(&key)
        .map(Some)
        .map_err(OrchestratorError::from)
}
