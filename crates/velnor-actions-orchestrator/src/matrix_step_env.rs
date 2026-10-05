//! Validated environment constructors for generated matrix steps.

use std::collections::BTreeMap;

use velnor_actions_contract::{RunsOn, SUPPORTED_TARGETS, target_for_runner_label};
use velnor_actions_mise::{ISOLATION_ENV, NO_AUTO_INSTALL_ENV, ToolCatalog, ToolHomes};
pub(crate) use velnor_actions_workflow_renderer::steps::{MBX_GC_AUTO_ENV, MBX_SHARE_OUT_DIR_ENV};
use velnor_actions_workflow_renderer::steps::{MBX_GC_AUTO_VALUE, MBX_SHARE_OUT_DIR_VALUE};

use super::OrchestratorError;

pub(crate) const MBX_CACHE_DIR_ENV: &str = "MBX_CACHE_DIR";
pub(crate) const MBX_CACHE_EXPORT_GROUP_ENV: &str = "MBX_CACHE_EXPORT_GROUP";
pub(crate) const MBX_CACHE_DIR_OUTPUT: &str = "${{ steps.mbx_cache_import.outputs.cache_dir }}";
pub(crate) const MBX_CACHE_EXPORT_GROUP_OUTPUT: &str =
    "${{ steps.mbx_cache_import.outputs.export_group }}";

/// Validated env every crate-job step runs with.
///
/// Single constructor shared by obligation steps and `Fetch Cargo
/// sources`: the isolation quartet plus install disable from the Mise
/// adapter's single source, the owned-homes triple for rust steps,
/// and caller extras. Tofu obligations carry no triple; they carry
/// the isolated per-root `TF_DATA_DIR` from the tofu adapter's
/// derivation instead (the temp CLI config path stays local-only
/// until a materialization step lands). Reserved keys in the extras
/// fail closed, so generated steps use the same validated contract as
/// local helper requests and fetch plus consumers can never drift
/// apart (run 36560676954 failed every leg when only `Run task`
/// carried the triple).
///
/// # Errors
///
/// Returns a contract error for reserved extras (triple keys included
/// when `needs_rust` is false) and a render error for blank triple
/// inputs or denied credential keys.
pub(crate) fn task_step_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
    needs_rust: bool,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_renderer::toolchain_env;

    for key in extra.keys() {
        let expected_mbx_value = match key.as_str() {
            MBX_CACHE_DIR_ENV => Some(MBX_CACHE_DIR_OUTPUT),
            MBX_CACHE_EXPORT_GROUP_ENV => Some(MBX_CACHE_EXPORT_GROUP_OUTPUT),
            MBX_GC_AUTO_ENV => Some(MBX_GC_AUTO_VALUE),
            MBX_SHARE_OUT_DIR_ENV => Some(MBX_SHARE_OUT_DIR_VALUE),
            _ => None,
        };
        if let Some(expected) = expected_mbx_value {
            if !needs_rust {
                return Err(OrchestratorError::Contract {
                    problem: format!("reserved_step_env:{key}"),
                });
            }
            if extra.get(key).map(String::as_str) != Some(expected) {
                return Err(OrchestratorError::Contract {
                    problem: format!("unvalidated_mbx_cache_env:{key}"),
                });
            }
            continue;
        }
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
    if let Some(data_dir) = super::tofu_env::tofu_data_dir_for_extra(extra)? {
        base.insert(velnor_actions_tofu::TF_DATA_DIR_ENV.to_owned(), data_dir);
    }
    if let Some(cache_dir) = super::tofu_env::tofu_plugin_cache_dir_for_extra(extra)? {
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
    let toolchain = catalog.rustup_toolchain();
    toolchain_env::checked_task_env(&base, homes.rustup_home(), homes.cargo_home(), &toolchain)
        .map_err(OrchestratorError::from)
}

/// Exact MBX env for one validated runner selector. Cache paths and group
/// come from this job's preflight; GC is disabled only on hosted Linux,
/// matching the renderer's job-level policy. Scale Sets keep MBX's default.
pub(super) fn mbx_producer_env(
    runner_label: &str,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut env = BTreeMap::from([
        (
            MBX_CACHE_DIR_ENV.to_owned(),
            MBX_CACHE_DIR_OUTPUT.to_owned(),
        ),
        (
            MBX_CACHE_EXPORT_GROUP_ENV.to_owned(),
            MBX_CACHE_EXPORT_GROUP_OUTPUT.to_owned(),
        ),
        (
            MBX_SHARE_OUT_DIR_ENV.to_owned(),
            MBX_SHARE_OUT_DIR_VALUE.to_owned(),
        ),
    ]);
    let runner = RunsOn::parse(runner_label).map_err(|err| OrchestratorError::Contract {
        problem: format!("invalid_runner_selector:{err}"),
    })?;
    if matches!(runner, RunsOn::Hosted(label) if target_for_runner_label(&label) == Some(SUPPORTED_TARGETS[0]))
    {
        env.insert(MBX_GC_AUTO_ENV.to_owned(), MBX_GC_AUTO_VALUE.to_owned());
    }
    Ok(env)
}
