// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
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

#[must_use]
pub fn declared_or_unreadable_in(directory: &Path) -> bool {
    let configs = configs_in(directory);
    configs.iter().any(|(_, table)| table.is_none()) || tables_declare(&configs).unwrap_or(true)
}

fn configs_in(directory: &Path) -> Vec<(&'static str, Option<Table>)> {
    CONFIGS
        .iter()
        .filter_map(|name| {
            let text = match std::fs::read_to_string(directory.join(name)) {
                Ok(text) => text,
                Err(error)
                    if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
                {
                    return None;
                }
                Err(_) => return Some((*name, None)),
            };
            Some((*name, text.parse::<Table>().ok()))
        })
        .collect()
}

pub fn checked_in(directory: &Path) -> Result<bool, String> {
    tables_declare(&configs_in(directory))
}

fn tables_declare(configs: &[(&str, Option<Table>)]) -> Result<bool, String> {
    let loaded: Vec<(&str, &Table)> = configs
        .iter()
        .filter_map(|(name, table)| Some((*name, table.as_ref()?)))
        .collect();
    let tables: Vec<&Table> = loaded.iter().map(|(_, table)| *table).collect();
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
