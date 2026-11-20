// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use toml::Table;
use velnor_model::{MbxRule, mbx_declaration};

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
    let tables: Vec<Table> = CONFIGS
        .iter()
        .filter_map(|name| {
            std::fs::read_to_string(directory.join(name))
                .ok()
                .and_then(|text| text.parse::<Table>().ok())
        })
        .collect();
    mbx_declaration(&tables, MbxRule::Wide).declared()
}
