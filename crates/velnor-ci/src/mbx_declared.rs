// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Whether the checkout declares that cargo runs through MBX: the owner standard layout is rust
//! `mr_boxington = true` in `[tools]` and a `[wrappers.cargo]` whose command is `mbx`. A pin of
//! the `mr-boxington` tool alone declares nothing, because nothing routes cargo through it.

use std::path::Path;
use toml::{Table, Value};

/// The mise config files a checkout root can carry, in mise's own lookup order.
const CONFIGS: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];

#[must_use]
pub fn declared_in(directory: &Path) -> bool {
    CONFIGS.iter().any(|name| {
        std::fs::read_to_string(directory.join(name))
            .ok()
            .and_then(|text| text.parse::<Table>().ok())
            .is_some_and(|table| declares(&table))
    })
}

fn declares(table: &Table) -> bool {
    let option = table
        .get("tools")
        .and_then(Value::as_table)
        .is_some_and(|tools| tools.values().any(tool_routes_through_mbx));
    let wrapper = table
        .get("wrappers")
        .and_then(Value::as_table)
        .and_then(|wrappers| wrappers.get("cargo"))
        .and_then(Value::as_table)
        .and_then(|cargo| cargo.get("command"))
        .and_then(Value::as_str)
        .and_then(|command| command.split_whitespace().next())
        .is_some_and(|program| {
            Path::new(program)
                .file_name()
                .is_some_and(|name| name == "mbx")
        });
    option || wrapper
}

fn tool_routes_through_mbx(tool: &Value) -> bool {
    match tool {
        Value::Table(options) => options.get("mr_boxington") == Some(&Value::Boolean(true)),
        Value::Array(entries) => entries.iter().any(tool_routes_through_mbx),
        _ => false,
    }
}
