// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

pub(crate) const REPO_MISE_CONFIG: &str = include_str!("../../../../mise.toml");
pub(crate) const REPO_MISE_LOCK: &str = include_str!("../../../../mise.lock");

pub(crate) fn repo_config_without_wrappers() -> Result<String> {
    let mut config: toml::Table = REPO_MISE_CONFIG.parse().context("parse mise.toml")?;
    config
        .remove("wrappers")
        .context("mise.toml has no [wrappers]")?;
    config
        .get_mut("tools")
        .and_then(toml::Value::as_table_mut)
        .and_then(|tools| tools.get_mut("rust"))
        .and_then(toml::Value::as_table_mut)
        .and_then(|rust| rust.remove("mr_boxington"))
        .context("the rust tool has no mr_boxington option")?;
    toml::to_string(&config).context("write mise.toml")
}
