//! Fixed tofu payload env per task kind (T03 baseline).
//!
//! Pure data like [`tofu_payload_argv`](crate::argv::tofu_payload_argv):
//! the orchestrator threads the returned pairs through the pinned-tool
//! execution env. Every kind carries the automation pair; T16 adds the
//! isolated per-root data dir plus the temp M4 CLI config path. T21
//! adds the job-private per-root plugin-cache dir.

use std::ffi::OsString;

use velnor_actions_contract::{ContractError, digest_b3};

use crate::kinds::TofuTaskKind;

/// Automation-marker env key.
pub const TF_IN_AUTOMATION_ENV: &str = "TF_IN_AUTOMATION";
/// Automation marker enabled.
pub const TF_IN_AUTOMATION_ON: &str = "1";
/// Interactive-input env key.
pub const TF_INPUT_ENV: &str = "TF_INPUT";
/// Interactive input disabled.
pub const TF_INPUT_OFF: &str = "0";
/// Isolated per-root data-dir env key (T16).
pub const TF_DATA_DIR_ENV: &str = "TF_DATA_DIR";
/// Isolated temp CLI-config env key (T16, M4).
pub const TF_CLI_CONFIG_FILE_ENV: &str = "TF_CLI_CONFIG_FILE";
/// Job-private per-root plugin-cache env key (T21 transport).
pub const TF_PLUGIN_CACHE_DIR_ENV: &str = "TF_PLUGIN_CACHE_DIR";
/// Max cache-dir bytes embedded in CLI config (headroom under OS limits).
pub const MAX_CLI_CONFIG_PATH_BYTES: usize = 1024;

/// Fixed payload env for one task kind.
///
/// Every kind carries `TF_IN_AUTOMATION=1 TF_INPUT=0` (T03 baseline
/// automation pair).
#[must_use]
pub fn tofu_payload_env(kind: TofuTaskKind) -> Vec<(OsString, OsString)> {
    let _ = kind;
    automation_pair()
}

/// T03 baseline automation pair both env builders share.
fn automation_pair() -> Vec<(OsString, OsString)> {
    vec![
        (
            OsString::from(TF_IN_AUTOMATION_ENV),
            OsString::from(TF_IN_AUTOMATION_ON),
        ),
        (OsString::from(TF_INPUT_ENV), OsString::from(TF_INPUT_OFF)),
    ]
}

/// Fixed tofu isolation env: the automation pair plus isolated
/// data/config/cache paths.
///
/// The single author of the pair structure: the Mise spawn
/// constructor bakes these same keys, and the orchestrator pins the
/// two outputs equal by test so rendered and spawned children never
/// drift apart.
#[must_use]
pub fn tofu_isolation_env(
    data_dir: &str,
    config_file: &str,
    cache_dir: &str,
) -> Vec<(OsString, OsString)> {
    let mut env = automation_pair();
    env.push((OsString::from(TF_DATA_DIR_ENV), OsString::from(data_dir)));
    env.push((
        OsString::from(TF_CLI_CONFIG_FILE_ENV),
        OsString::from(config_file),
    ));
    env.push((
        OsString::from(TF_PLUGIN_CACHE_DIR_ENV),
        OsString::from(cache_dir),
    ));
    env
}

/// `b3-` plus 64 lowercase hexadecimal digits, bound to one exact root.
///
/// Semantic identity stays reversible in the `dir-<hex>` task key. This
/// opaque locator keeps data and provider-cache paths bounded.
/// # Errors
/// Rejects noncanonical normalized roots.
pub fn tofu_root_locator(root: &str) -> Result<String, ContractError> {
    crate::validate_normalized_root(root)?;
    let key = crate::key_for_root(root);
    Ok(digest_b3(format!("tofu-private-root-v1\n{key}").as_bytes()))
}

/// Isolated per-root data dir under `base`.
///
/// `{base}/` plus the shared [`tofu_root_locator`]. `base` is
/// caller-owned (a runner-temp expression for rendered steps, a
/// staging dir for local spawns).
///
/// # Errors
///
/// Returns [`ContractError`] for an empty base.
pub fn tofu_data_dir_under(base: &str, root: &str) -> Result<String, ContractError> {
    if base.is_empty() {
        return Err(ContractError::identity("tofu_data_dir", "empty_base"));
    }
    Ok(format!("{base}/{}", tofu_root_locator(root)?))
}

/// Job-private per-root plugin-cache dir under `base` (T21).
///
/// `{base}/` plus the shared [`tofu_root_locator`]: the data dir and
/// the plugin-cache dir for one root share the locator but never the
/// base, so the two stay separate by construction.
///
/// # Errors
///
/// Returns [`ContractError`] for an empty base.
pub fn tofu_cache_dir_under(base: &str, root: &str) -> Result<String, ContractError> {
    if base.is_empty() {
        return Err(ContractError::identity("tofu_cache_dir", "empty_base"));
    }
    Ok(format!("{base}/{}", tofu_root_locator(root)?))
}

/// M4 CLI config content: private cache, checkpoint disable, direct providers.
///
/// No credentials, helpers, overrides, or mirrors. Explicit `direct {}`
/// disables implied filesystem mirrors. The cache dir is the sole
/// interpolated value; reject HCL breakouts, empty paths, and paths over
/// [`MAX_CLI_CONFIG_PATH_BYTES`].
///
/// # Errors
///
/// Returns [`ContractError`] for empty, oversize, or HCL-unsafe input.
pub fn tofu_cli_config(cache_dir: &str) -> Result<String, ContractError> {
    if cache_dir.is_empty() {
        return Err(ContractError::identity(
            "tofu_cli_config",
            "empty_cache_dir",
        ));
    }
    if cache_dir.len() > MAX_CLI_CONFIG_PATH_BYTES {
        return Err(ContractError::identity(
            "tofu_cli_config",
            "oversize_cache_dir",
        ));
    }
    if cache_dir.contains("${") {
        return Err(ContractError::identity(
            "tofu_cli_config",
            "interpolation_cache_dir",
        ));
    }
    if let Some(bad) = cache_dir
        .bytes()
        .find(|byte| *byte == b'"' || *byte == b'\\' || *byte < 0x20 || *byte == 0x7f)
    {
        return Err(ContractError::identity(
            "tofu_cli_config",
            format!("rejected_byte:{bad:02x}"),
        ));
    }
    Ok(format!(
        "plugin_cache_dir = \"{cache_dir}\"\ndisable_checkpoint = true\nprovider_installation {{\n  direct {{}}\n}}\n"
    ))
}
