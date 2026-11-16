// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::RepoPath;

use crate::error::DetectError;
use crate::roots::is_cargo_config;
use crate::toml_file::table;

pub fn repo_profile(files: &BTreeMap<RepoPath, Vec<u8>>) -> Result<String, DetectError> {
    let mut sections = Vec::new();
    for (file, bytes) in files.iter().filter(|(file, _)| is_cargo_config(file)) {
        let shadowed = file
            .as_str()
            .strip_suffix(".toml")
            .and_then(|name| RepoPath::new(name).ok())
            .is_some_and(|name| files.contains_key(&name));
        if shadowed {
            continue;
        }
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        if let Some(profile) = parsed.get("profile") {
            let wrapped = toml::Table::from_iter([("profile".to_owned(), profile.clone())]);
            sections.push(format!("{file}\n{wrapped}\n"));
        }
    }
    Ok(sections.concat())
}
