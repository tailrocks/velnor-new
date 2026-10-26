// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{
    cache_family::{
        TOFU_PROVIDERS_KEY_OUTPUT_PREFIX, TOFU_PROVIDERS_SAVE_CONDITION, is_plan_cache_output,
    },
    step::{Step, StepKind},
    step_identity::{
        StepId, StepRole, TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR,
        TOFU_PROVIDERS_KEY_OUTPUT_EXPR, TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
    },
};
use crate::errors::ContractError;

#[derive(Default)]
struct ProtocolSteps {
    restores: Vec<usize>,
    consumers: Vec<usize>,
    saves: Vec<usize>,
}

struct ProviderBinding<'a> {
    key: &'a str,
    path: &'a str,
    restore_index: usize,
}

pub(super) fn validate_tofu_provider_sequence(
    steps: &[Step],
    scope: &str,
) -> Result<(), ContractError> {
    let roles = classify_protocol_steps(steps, scope)?;
    if !roles.present() {
        return Ok(());
    }
    let binding = validate_restore_admission(steps, &roles, scope)?;
    validate_provider_consumers(steps, &roles.consumers, &binding, scope)?;
    validate_provider_save(steps, &roles, &binding, scope)
}

fn classify_protocol_steps(steps: &[Step], scope: &str) -> Result<ProtocolSteps, ContractError> {
    let mut roles = ProtocolSteps::default();
    for (index, step) in steps.iter().enumerate() {
        match step.role {
            Some(StepRole::TofuProvidersRestore) => roles.restores.push(index),
            Some(StepRole::TofuProviderUse) => roles.consumers.push(index),
            Some(StepRole::TofuProvidersSave) => roles.saves.push(index),
            _ => {}
        }
        let uses_provider_env = has_provider_env(step);
        if uses_provider_env && step.role != Some(StepRole::TofuProviderUse) {
            return Err(invalid(scope, "tofu_provider_use_role_missing"));
        }
        if is_provider_cache_action(step)
            && !matches!(
                step.role,
                Some(StepRole::TofuProvidersRestore | StepRole::TofuProvidersSave)
            )
        {
            return Err(invalid(scope, "tofu_provider_cache_role_missing"));
        }
    }
    Ok(roles)
}

impl ProtocolSteps {
    fn present(&self) -> bool {
        !self.restores.is_empty() || !self.consumers.is_empty() || !self.saves.is_empty()
    }
}

fn validate_restore_admission<'a>(
    steps: &'a [Step],
    roles: &ProtocolSteps,
    scope: &str,
) -> Result<ProviderBinding<'a>, ContractError> {
    if roles.restores.len() != 1 {
        return Err(invalid(scope, "tofu_provider_restore_count"));
    }
    let restore_index = roles.restores[0];
    let restore = &steps[restore_index];
    if restore.condition.is_some() {
        return Err(invalid(scope, "tofu_provider_restore_conditional"));
    }
    let restore_with = action_with(restore).ok_or_else(|| invalid(scope, "tofu_restore_shape"))?;
    let key = restore_with
        .get("cache-key")
        .map(String::as_str)
        .ok_or_else(|| invalid(scope, "tofu_restore_key_missing"))?;
    let path = restore_with
        .get("cache-path")
        .map(String::as_str)
        .ok_or_else(|| invalid(scope, "tofu_restore_path_missing"))?;
    provider_path_slug(path).ok_or_else(|| invalid(scope, "tofu_provider_path_invalid"))?;
    if !valid_provider_cache_key(key) {
        return Err(invalid(scope, "tofu_provider_key_invalid"));
    }
    Ok(ProviderBinding {
        key,
        path,
        restore_index,
    })
}

fn validate_provider_consumers(
    steps: &[Step],
    consumers: &[usize],
    binding: &ProviderBinding<'_>,
    scope: &str,
) -> Result<(), ContractError> {
    for index in consumers.iter().copied() {
        let consumer = &steps[index];
        let env = step_env(consumer).ok_or_else(|| invalid(scope, "tofu_provider_use_env"))?;
        if index <= binding.restore_index {
            return Err(invalid(scope, "tofu_provider_use_before_admission"));
        }
        if env.get("TF_PLUGIN_CACHE_DIR").map(String::as_str) != Some(binding.path) {
            return Err(invalid(scope, "tofu_provider_use_path_mismatch"));
        }
        if env.get("TF_DATA_DIR").is_none_or(|data_dir| {
            data_dir == binding.path || data_dir.starts_with(&format!("{}/", binding.path))
        }) {
            return Err(invalid(scope, "tofu_data_dir_provider_cache_alias"));
        }
    }
    Ok(())
}

fn validate_provider_save(
    steps: &[Step],
    roles: &ProtocolSteps,
    binding: &ProviderBinding<'_>,
    scope: &str,
) -> Result<(), ContractError> {
    if roles.saves.len() > 1 {
        return Err(invalid(scope, "tofu_provider_save_count"));
    }
    if let Some(index) = roles.saves.first().copied() {
        if index <= binding.restore_index {
            return Err(invalid(scope, "tofu_provider_save_before_restore"));
        }
        let save = &steps[index];
        let save_with = action_with(save).ok_or_else(|| invalid(scope, "tofu_save_shape"))?;
        let restore = &steps[binding.restore_index];
        let restore_with =
            action_with(restore).ok_or_else(|| invalid(scope, "tofu_restore_shape"))?;
        if restore_with.get("cache-key").map(String::as_str) != Some(binding.key)
            || restore_with.get("cache-path").map(String::as_str) != Some(binding.path)
        {
            return Err(invalid(scope, "tofu_provider_save_binding_mismatch"));
        }
        let expected_key = output_reference(restore, "cache-key")
            .ok_or_else(|| invalid(scope, "tofu_provider_restore_id_missing"))?;
        let expected_path = output_reference(restore, "cache-path")
            .ok_or_else(|| invalid(scope, "tofu_provider_restore_id_missing"))?;
        if save_with.get("key") != Some(&expected_key)
            || save_with.get("path") != Some(&expected_path)
        {
            return Err(invalid(scope, "tofu_provider_save_binding_mismatch"));
        }
        if save.condition.as_deref() != Some(TOFU_PROVIDERS_SAVE_CONDITION) {
            return Err(invalid(scope, "tofu_provider_save_gate_mismatch"));
        }
        if roles.consumers.iter().any(|consumer| index <= *consumer) {
            return Err(invalid(scope, "tofu_provider_save_before_use"));
        }
    }
    Ok(())
}

fn output_reference(step: &Step, output: &str) -> Option<String> {
    let id = step.id?;
    Some(format!("${{{{ steps.{}.outputs.{output} }}}}", id.as_str()))
}

pub(crate) fn valid_provider_restore(kind: &StepKind) -> bool {
    let StepKind::Action { uses, with, env } = kind else {
        return false;
    };
    let Some(key) = with.get("cache-key") else {
        return false;
    };
    let Some(path) = with.get("cache-path") else {
        return false;
    };
    with.len() == 2
        && uses == TOFU_PROVIDER_ADMISSION_USES
        && env.is_empty()
        && provider_path_slug(path).is_some()
        && valid_provider_cache_key(key)
}

pub(crate) fn valid_provider_save(kind: &StepKind) -> bool {
    let StepKind::Action { uses, with, env } = kind else {
        return false;
    };
    let Some(key) = with.get("key") else {
        return false;
    };
    let Some(path) = with.get("path") else {
        return false;
    };
    with.len() == 2
        && uses.starts_with("actions/cache/save@")
        && env.is_empty()
        && key == TOFU_PROVIDERS_KEY_OUTPUT_EXPR
        && path == TOFU_PROVIDERS_PATH_OUTPUT_EXPR
}

pub(crate) fn valid_provider_use(kind: &StepKind) -> bool {
    let StepKind::Shell { env, .. } = kind else {
        return false;
    };
    let Some(path) = env.get("TF_PLUGIN_CACHE_DIR") else {
        return false;
    };
    provider_path_slug(path).is_some()
        && env.get("TF_DATA_DIR").is_some_and(|data_dir| {
            !data_dir.is_empty() && data_dir != path && !data_dir.starts_with(&format!("{path}/"))
        })
}

fn provider_path_slug(path: &str) -> Option<&str> {
    let slug = path.strip_prefix(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/"))?;
    (!slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(slug)
}

fn valid_provider_cache_key(key: &str) -> bool {
    key.strip_prefix("${{needs.plan.outputs.")
        .and_then(|rest| rest.strip_suffix("}}"))
        .is_some_and(|name| {
            name.starts_with(TOFU_PROVIDERS_KEY_OUTPUT_PREFIX) && is_plan_cache_output(name)
        })
}

fn has_provider_env(step: &Step) -> bool {
    step_env(step).is_some_and(|env| {
        env.contains_key("TF_PLUGIN_CACHE_DIR") || env.contains_key("TF_DATA_DIR")
    })
}

fn is_provider_cache_action(step: &Step) -> bool {
    if step.id == Some(StepId::TofuProviders) {
        return true;
    }
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    if uses == TOFU_PROVIDER_ADMISSION_USES {
        return true;
    }
    let cache_action =
        uses.starts_with("actions/cache/restore@") || uses.starts_with("actions/cache/save@");
    cache_action
        && (with
            .get("key")
            .is_some_and(|key| valid_provider_cache_key(key))
            || with
                .get("path")
                .is_some_and(|path| path.contains(TOFU_PROVIDER_CACHE_BASE_EXPR)))
}

fn action_with(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { with, .. } => Some(with),
        StepKind::Shell { .. }
        | StepKind::TaskExecution { .. }
        | StepKind::NativeCheckExecution { .. }
        | StepKind::Internal { .. } => None,
    }
}

fn step_env(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::TaskExecution { env, .. } => Some(env),
        StepKind::NativeCheckExecution { .. } | StepKind::Internal { .. } => None,
    }
}

fn invalid(scope: &str, problem: &str) -> ContractError {
    ContractError::identity(
        "job.steps.tofu_provider_cache",
        format!("{problem}:{scope}"),
    )
}

pub(crate) fn valid_playwright_install(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Internal { operation, .. }
        if operation == crate::config::playwright_lock::PLAYWRIGHT_INSTALL_OPERATION)
}
