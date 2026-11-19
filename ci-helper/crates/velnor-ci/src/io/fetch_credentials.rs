// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::CiError;

const BUILT_IN: &str = "cargo:token";
const GLOBAL_LAST: &str =
    "registry.global-credential-providers=[\"cargo:token-from-stdout /usr/bin/true\"]";
const CRATES_IO_VARIABLE: &str = "CARGO_REGISTRY_CREDENTIAL_PROVIDER";
const MAX_INCLUDE_DEPTH: usize = 8;

#[derive(Default)]
pub(super) struct Neutralised {
    pub(super) config: Vec<&'static str>,
    pub(super) environment: Vec<(OsString, OsString)>,
}

#[derive(Default)]
struct Declared {
    registries: BTreeSet<String>,
    crates_io: bool,
    global: bool,
    seen: BTreeSet<PathBuf>,
}

pub(super) fn neutralise(directory: &Path, cargo_home: &Path) -> Result<Neutralised, CiError> {
    let mut declared = Declared::default();
    let mut starts = vec![std::path::absolute(directory).unwrap_or_else(|_| directory.into())];
    if let Ok(real) = std::fs::canonicalize(directory) {
        starts.push(real);
    }
    for start in &starts {
        for ancestor in start.ancestors() {
            declared.scan_directory(&ancestor.join(".cargo"));
        }
    }
    declared.scan_directory(cargo_home);
    let mut neutralised = Neutralised::default();
    if declared.global {
        neutralised.config.push(GLOBAL_LAST);
    }
    if declared.crates_io {
        neutralised
            .environment
            .push((CRATES_IO_VARIABLE.into(), BUILT_IN.into()));
    }
    for name in &declared.registries {
        neutralised
            .environment
            .push((registry_variable(name)?.into(), BUILT_IN.into()));
    }
    Ok(neutralised)
}

fn registry_variable(name: &str) -> Result<String, CiError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CiError::new(format!(
            "the Cargo registry name {name:?} cannot be given a built-in credential provider; refusing to run with the token"
        )));
    }
    Ok(format!(
        "CARGO_REGISTRIES_{}_CREDENTIAL_PROVIDER",
        name.to_ascii_uppercase().replace('-', "_")
    ))
}

impl Declared {
    fn scan_directory(&mut self, directory: &Path) {
        for name in ["config.toml", "config"] {
            self.scan(&directory.join(name), 0);
        }
    }

    fn scan(&mut self, path: &Path, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH || !self.seen.insert(path.to_path_buf()) {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(table) = text.parse::<toml::Table>() else {
            return;
        };
        if let Some(registries) = table.get("registries").and_then(toml::Value::as_table) {
            self.registries.extend(registries.keys().cloned());
        }
        if let Some(registry) = table.get("registry").and_then(toml::Value::as_table) {
            self.crates_io |= registry.contains_key("credential-provider");
            self.global |= registry.contains_key("global-credential-providers");
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let includes = table.get("include").and_then(toml::Value::as_array);
        for include in includes.into_iter().flatten() {
            let target = include
                .as_str()
                .or_else(|| include.get("path").and_then(toml::Value::as_str));
            if let Some(target) = target {
                self.scan(&base.join(target), depth + 1);
            }
        }
    }
}
