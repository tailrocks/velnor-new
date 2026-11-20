// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use toml::Table;
use velnor_model::{MbxDeclaration, MbxRule, mbx_declaration};

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
    checked_in(directory).unwrap_or(true)
}

pub fn checked_in(directory: &Path) -> Result<bool, String> {
    let loaded: Vec<(&str, Table)> = CONFIGS
        .iter()
        .filter_map(|name| {
            let table = std::fs::read_to_string(directory.join(name))
                .ok()?
                .parse::<Table>()
                .ok()?;
            Some((*name, table))
        })
        .collect();
    let tables: Vec<&Table> = loaded.iter().map(|(_, table)| table).collect();
    match mbx_declaration(tables, MbxRule::Wide) {
        MbxDeclaration::Conflicting(conflict) => {
            let name = |index: usize| loaded.get(index).map_or("?", |(name, _)| *name);
            Err(format!(
                "{} and {} both set {}; keep one of them so velnor-ci can tell whether MBX is declared",
                name(conflict.first),
                name(conflict.second),
                conflict.key
            ))
        }
        declaration => Ok(declaration.declared()),
    }
}
