// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::OrchestratorError;
use crate::task_report::TASK_ID_ENV;
use velnor_actions_contract::{
    CrateObligation, Stack, Step, StepKind, StepRole, sanitize_error_detail,
};
use velnor_actions_mise::{ISOLATION_ENV, NO_AUTO_INSTALL_ENV, ToolCatalog, ToolHomes};
use velnor_actions_rust::{payload_env_for_kind, step_base_name};
use velnor_actions_workflow_renderer::plan_format::FORMAT_STEP_NAME;

#[path = "matrix_tools.rs"]
mod tools;

#[path = "matrix_tofu_env.rs"]
mod tofu_env;

pub(crate) use tools::{
    CrateSuite, crate_needs_generate_validators, prepare_crate_tools_step,
    prepare_install_opentofu, suite_for_package,
};

pub(crate) const OBLIGATION_TASK_ID_ENV: &str = "VELNOR_TASK_ID";
pub(crate) const OBLIGATION_TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
pub(crate) const OBLIGATION_MATRIX_ID_ENV: &str = "VELNOR_MATRIX_ID";
pub(crate) const OBLIGATION_MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";

pub(crate) fn step_name_for(kind: &str, task_id: &str) -> String {
    let base = match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::step_base_name(kind, FORMAT_STEP_NAME),
        _ => step_base_name(kind, FORMAT_STEP_NAME),
    };
    match shard_suffix(task_id) {
        Some((index, count)) => format!("{base} (shard {index} of {count})"),
        None => base.to_owned(),
    }
}

pub(crate) fn shard_suffix(task_id: &str) -> Option<(u32, u32)> {
    let (_, index, count) = velnor_actions_contract::split_shard_suffix(task_id)?;
    Some((index, count))
}

fn obligation_stack(task_id: &str) -> Option<Stack> {
    crate::extension_schemas::task_stack_segment(task_id).and_then(Stack::from_id)
}

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";

fn payload_env_for_obligation(task_id: &str, kind: &str) -> Vec<(OsString, OsString)> {
    match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::payload_env_for_kind(kind),
        _ => payload_env_for_kind(kind),
    }
}

pub(crate) fn task_step_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
    needs_rust: bool,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_renderer::toolchain_env;
    for key in extra.keys() {
        if velnor_actions_mise::command::is_reserved_env_key(key)
            || (!needs_rust && toolchain_env::TOOLCHAIN_HOME_KEYS.contains(&key.as_str()))
        {
            return Err(OrchestratorError::Contract {
                problem: format!("reserved_step_env:{key}"),
            });
        }
    }
    let mut base = BTreeMap::new();
    for (key, value) in ISOLATION_ENV.iter().chain(NO_AUTO_INSTALL_ENV.iter()) {
        base.insert((*key).to_owned(), (*value).to_owned());
    }
    base.extend(
        extra
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    if let Some(data_dir) = tofu_env::tofu_data_dir_for_extra(extra)? {
        base.insert(velnor_actions_tofu::TF_DATA_DIR_ENV.to_owned(), data_dir);
    }
    if let Some(cache_dir) = tofu_env::tofu_plugin_cache_dir_for_extra(extra)? {
        base.insert(
            velnor_actions_tofu::TF_PLUGIN_CACHE_DIR_ENV.to_owned(),
            cache_dir,
        );
    }
    if !needs_rust {
        toolchain_env::reject_denied_step_keys(&base).map_err(OrchestratorError::from)?;
        return Ok(base);
    }
    let homes = ToolHomes::runner_temp();
    let toolchain = catalog.rustup_toolchain_env().unwrap_or("");
    toolchain_env::checked_task_env(&base, homes.rustup_home(), homes.cargo_home(), toolchain)
        .map_err(OrchestratorError::from)
}

pub(crate) fn obligation_identity_env(
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
    matrix_cap: Option<u32>,
) -> BTreeMap<String, String> {
    let mut identity = BTreeMap::from([
        (OBLIGATION_TASK_ID_ENV.to_owned(), task_id.to_owned()),
        (
            OBLIGATION_TASK_DIGEST_ENV.to_owned(),
            task_digest.to_owned(),
        ),
        (OBLIGATION_MATRIX_ID_ENV.to_owned(), matrix_id.to_owned()),
        (OBLIGATION_MATRIX_KEY_ENV.to_owned(), matrix_key.to_owned()),
    ]);
    if let Some(cap) = matrix_cap {
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_NEEDS_JOB_ENV.to_owned(),
            velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned(),
        );
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_OUTPUT_ENV.to_owned(),
            velnor_actions_workflow_renderer::PLAN_ID_OUTPUT.to_owned(),
        );
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_MAX_PARALLEL_ENV.to_owned(),
            cap.to_string(),
        );
    }
    identity
}

pub(crate) fn obligation_step(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    _downstream: &[String],
    matrix_cap: Option<u32>,
    helper_version: &str,
    target_lane: Option<&str>,
) -> Result<Step, OrchestratorError> {
    let stack_id = obligation_stack(&obligation.task_id).map_or(Stack::Rust.id(), Stack::id);
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(stack_id, &obligation.task_id)
            .map_err(crate::internal::internal_contract)?;
    let needs_rust = obligation_stack(&obligation.task_id) != Some(Stack::Tofu);
    let mut payload_env = BTreeMap::new();
    for (key, value) in payload_env_for_obligation(&obligation.task_id, &obligation.kind) {
        payload_env.insert(
            key.to_string_lossy().into_owned(),
            value.to_string_lossy().into_owned(),
        );
    }
    if let Some(lane) = target_lane {
        if !needs_rust {
            return Err(crate::internal::internal("tofu_target_lane"));
        }
        payload_env.insert(CARGO_TARGET_DIR_ENV.to_owned(), lane.to_owned());
    }
    let step = if needs_rust {
        let env = task_step_env(catalog, &payload_env, true)?;
        Step {
            parallel: false,
            name: obligation.step_name.clone(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::TaskExecution {
                argv: obligation.run.clone(),
                env,
                task_id: obligation.task_id.clone(),
                task_digest: obligation.task_digest.clone(),
                toolchain_inputs: obligation.toolchain_inputs.clone(),
                matrix_id,
                matrix_key: obligation.matrix_key.clone(),
                report_helper_version: helper_version.to_owned(),
                matrix_max_parallel: matrix_cap,
            },
        }
    } else {
        let mut identity = obligation_identity_env(
            &obligation.task_id,
            &obligation.task_digest,
            &matrix_id,
            &obligation.matrix_key,
            matrix_cap,
        );
        identity.extend(payload_env);
        check_identity_env_contract(&identity, &obligation.task_id)?;
        let env = task_step_env(catalog, &identity, false)?;
        let joined = velnor_actions_workflow_renderer::join_argv_for_run(&obligation.run).map_err(
            |err| OrchestratorError::Contract {
                problem: err.to_string(),
            },
        )?;
        let run = report_wrapper_argv(&joined, &helper_path_for_version(helper_version));
        let mut tofu =
            velnor_actions_workflow_renderer::shell_step(&obligation.step_name, run, env)
                .map_err(OrchestratorError::from)?;
        tofu.role = Some(StepRole::TofuProviderUse);
        tofu
    };
    Ok(step)
}

pub(crate) fn check_identity_env_contract(
    identity: &BTreeMap<String, String>,
    task_id: &str,
) -> Result<(), OrchestratorError> {
    if identity.get(TASK_ID_ENV).map(String::as_str) != Some(task_id) {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "obligation_identity_mismatch:{}",
                sanitize_error_detail(task_id)
            ),
        });
    }
    Ok(())
}

#[path = "matrix_step_reports.rs"]
mod matrix_step_reports;
pub(crate) use matrix_step_reports::{
    crate_upload_step, deferred_report_argv, download_plan_step, helper_path_for_version,
    outcome_path_for_key, report_wrapper_argv, start_path_for_key,
};
