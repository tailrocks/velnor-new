// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{ModeBudget, Selection, SpeedBudget};

use crate::error::Reason;
use crate::toml_file::table;

const KEYS: [&str; 4] = [
    "pr_wall_seconds",
    "main_wall_seconds",
    "runner_seconds",
    "setup_seconds",
];

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    #[serde(rename = "pr_wall_seconds")]
    pr_wall: Option<u64>,
    #[serde(rename = "main_wall_seconds")]
    main_wall: Option<u64>,
    #[serde(rename = "runner_seconds")]
    runner: Option<u64>,
    #[serde(rename = "setup_seconds")]
    setup: Option<u64>,
}

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
    full: Option<RawLimits>,
    affected: Option<RawLimits>,
    none: Option<RawLimits>,
}

impl RawLimits {
    fn mode(&self) -> ModeBudget {
        ModeBudget {
            pr_wall_seconds: self.pr_wall,
            main_wall_seconds: self.main_wall,
            runner_seconds: self.runner,
            setup_seconds: self.setup,
        }
    }
}

impl RawBudget {
    fn budget(&self) -> SpeedBudget {
        let mode = |table: &Option<RawLimits>| table.as_ref().map_or_default(RawLimits::mode);
        SpeedBudget {
            pr_wall_seconds: self.pr_wall.unwrap_or(0),
            main_wall_seconds: self.main_wall.unwrap_or(0),
            runner_seconds: self.runner.unwrap_or(0),
            setup_seconds: self.setup.unwrap_or(0),
            full: mode(&self.full),
            affected: mode(&self.affected),
            none: mode(&self.none),
        }
    }
}

fn unknown_key(value: &toml::Value) -> Option<String> {
    let table = value.as_table()?;
    for (key, inner) in table {
        if let Some(mode) = Selection::from_name(key) {
            let unknown = inner
                .as_table()
                .and_then(|inner| inner.keys().find(|name| !KEYS.contains(&name.as_str())));
            if let Some(name) = unknown {
                return Some(format!("{}.{name}", mode.name()));
            }
        } else if !KEYS.contains(&key.as_str()) {
            return Some(key.clone());
        }
    }
    None
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Option<SpeedBudget>, Reason> {
    let table = table(bytes)?;
    let Some(value) = table.get("budget") else {
        return Ok(None);
    };
    if let Some(key) = unknown_key(value) {
        return Err(Reason::UnknownKey(format!("budget.{key}")));
    }
    let raw: RawBudget = value
        .clone()
        .try_into()
        .map_err(|error: toml::de::Error| Reason::Syntax(error.message().to_owned()))?;
    Ok(Some(raw.budget()))
}
