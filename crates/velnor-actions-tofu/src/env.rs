// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use velnor_actions_contract::{ContractError, digest_b3};

use crate::kinds::TofuTaskKind;

pub const TF_IN_AUTOMATION_ENV: &str = "TF_IN_AUTOMATION";
pub const TF_IN_AUTOMATION_ON: &str = "1";
pub const TF_INPUT_ENV: &str = "TF_INPUT";
pub const TF_INPUT_OFF: &str = "0";
pub const TF_DATA_DIR_ENV: &str = "TF_DATA_DIR";
pub const TF_CLI_CONFIG_FILE_ENV: &str = "TF_CLI_CONFIG_FILE";
pub const TF_PLUGIN_CACHE_DIR_ENV: &str = "TF_PLUGIN_CACHE_DIR";
pub const MAX_CLI_CONFIG_PATH_BYTES: usize = 1024;

#[must_use]
pub fn tofu_payload_env(kind: TofuTaskKind) -> Vec<(OsString, OsString)> {
    let _ = kind;
    automation_pair()
}

fn automation_pair() -> Vec<(OsString, OsString)> {
    vec![
        (
            OsString::from(TF_IN_AUTOMATION_ENV),
            OsString::from(TF_IN_AUTOMATION_ON),
        ),
        (OsString::from(TF_INPUT_ENV), OsString::from(TF_INPUT_OFF)),
    ]
}

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

pub fn tofu_root_locator(root: &str) -> Result<String, ContractError> {
    crate::validate_normalized_root(root)?;
    let key = crate::key_for_root(root);
    Ok(digest_b3(format!("tofu-private-root-v1\n{key}").as_bytes()))
}

pub fn tofu_data_dir_under(base: &str, root: &str) -> Result<String, ContractError> {
    if base.is_empty() {
        return Err(ContractError::identity("tofu_data_dir", "empty_base"));
    }
    Ok(format!("{base}/{}", tofu_root_locator(root)?))
}

pub fn tofu_cache_dir_under(base: &str, root: &str) -> Result<String, ContractError> {
    if base.is_empty() {
        return Err(ContractError::identity("tofu_cache_dir", "empty_base"));
    }
    Ok(format!("{base}/{}", tofu_root_locator(root)?))
}

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
