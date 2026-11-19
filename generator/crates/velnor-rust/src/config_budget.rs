// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::SpeedBudget;

use crate::error::Reason;
use crate::toml_file::table;

const KEYS: [&str; 4] = [
    "pr_wall_seconds",
    "main_wall_seconds",
    "runner_seconds",
    "setup_seconds",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBudget {
    #[serde(rename = "pr_wall_seconds")]
    pr_wall: Option<u64>,
    #[serde(rename = "main_wall_seconds")]
    main_wall: Option<u64>,
    #[serde(rename = "runner_seconds")]
    runner: Option<u64>,
    #[serde(rename = "setup_seconds")]
    setup: Option<u64>,
}

impl RawBudget {
    fn budget(&self) -> SpeedBudget {
        SpeedBudget {
            pr_wall_seconds: self.pr_wall.unwrap_or(0),
            main_wall_seconds: self.main_wall.unwrap_or(0),
            runner_seconds: self.runner.unwrap_or(0),
            setup_seconds: self.setup.unwrap_or(0),
        }
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Option<SpeedBudget>, Reason> {
    let table = table(bytes)?;
    let Some(value) = table.get("budget") else {
        return Ok(None);
    };
    if let Some(key) = value
        .as_table()
        .and_then(|table| table.keys().find(|key| !KEYS.contains(&key.as_str())))
    {
        return Err(Reason::UnknownKey(format!("budget.{key}")));
    }
    let raw: RawBudget = value
        .clone()
        .try_into()
        .map_err(|error: toml::de::Error| Reason::Syntax(error.message().to_owned()))?;
    Ok(Some(raw.budget()))
}
