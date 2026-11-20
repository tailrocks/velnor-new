// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::{Table, Value};

pub const MBX_TOOL: &str = "mr-boxington";
const MBX_TOOL_COMMAND: &str = "mbx";
const RUST_TOOLS: &[&str] = &["rust", "core:rust"];
const MBX_KEYS: &[&str] = &["tools.rust", "wrappers.cargo", "tools.mr-boxington"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxRule {
    Strict,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MbxConflict {
    pub key: &'static str,
    pub first: usize,
    pub second: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxDeclaration {
    Absent,
    Declared,
    Conflicting(MbxConflict),
}

impl MbxDeclaration {
    #[must_use]
    pub const fn declared(self) -> bool {
        !matches!(self, Self::Absent)
    }
}

#[must_use]
pub fn mbx_declaration<'a>(
    configs: impl IntoIterator<Item = &'a Table>,
    rule: MbxRule,
) -> MbxDeclaration {
    let configs: Vec<&Table> = configs.into_iter().collect();
    let mut owner: Option<usize> = None;
    for (index, config) in configs.iter().enumerate() {
        let Some(key) = MBX_KEYS.iter().copied().find(|key| sets(config, key)) else {
            continue;
        };
        match owner {
            None => owner = Some(index),
            Some(first) => {
                return MbxDeclaration::Conflicting(MbxConflict {
                    key,
                    first,
                    second: index,
                });
            }
        }
    }
    if configs.iter().any(|config| declares(config, rule)) {
        MbxDeclaration::Declared
    } else {
        MbxDeclaration::Absent
    }
}

fn tools(config: &Table) -> Option<&Table> {
    config.get("tools").and_then(Value::as_table)
}

fn sets(config: &Table, key: &str) -> bool {
    match key {
        "tools.rust" => tools(config)
            .is_some_and(|tools| RUST_TOOLS.iter().any(|name| tools.contains_key(*name))),
        "tools.mr-boxington" => tools(config).is_some_and(|tools| tools.contains_key(MBX_TOOL)),
        _ => config
            .get("wrappers")
            .and_then(Value::as_table)
            .is_some_and(|wrappers| wrappers.contains_key("cargo")),
    }
}

fn declares(config: &Table, rule: MbxRule) -> bool {
    match rule {
        MbxRule::Strict => strict_option(config) || strict_wrapper(config),
        MbxRule::Wide => wide_option(config) || wide_wrapper(config),
    }
}

fn wrapper_command(config: &Table) -> Option<&str> {
    config
        .get("wrappers")
        .and_then(Value::as_table)
        .and_then(|wrappers| wrappers.get("cargo"))
        .and_then(Value::as_table)
        .and_then(|cargo| cargo.get("command"))
        .and_then(Value::as_str)
}

fn strict_wrapper(config: &Table) -> bool {
    wrapper_command(config) == Some(MBX_TOOL_COMMAND)
}

fn wide_wrapper(config: &Table) -> bool {
    wrapper_command(config)
        .and_then(|command| command.split_whitespace().next())
        .is_some_and(|program| {
            std::path::Path::new(program)
                .file_name()
                .is_some_and(|name| name == MBX_TOOL_COMMAND)
        })
}

fn strict_option(config: &Table) -> bool {
    tools(config).is_some_and(|tools| {
        RUST_TOOLS
            .iter()
            .filter_map(|name| tools.get(*name))
            .any(option_table)
    })
}

fn wide_option(config: &Table) -> bool {
    tools(config).is_some_and(|tools| tools.values().any(routes_through_mbx))
}

fn option_table(tool: &Value) -> bool {
    tool.as_table()
        .is_some_and(|options| options.get("mr_boxington") == Some(&Value::Boolean(true)))
}

fn routes_through_mbx(tool: &Value) -> bool {
    match tool {
        Value::Array(entries) => entries.iter().any(routes_through_mbx),
        other => option_table(other),
    }
}
