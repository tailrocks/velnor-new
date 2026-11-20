// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::Path;

use toml::{Table, Value};

use crate::error::CiError;

const CONFIGS: [&str; 2] = ["mise.toml", ".mise.toml"];
const LOCK: &str = "mise.lock";
const OUTPUT_VARIABLE: &str = "GITHUB_OUTPUT";

pub fn pinned_version(dir: &Path, tool: &str) -> Result<String, CiError> {
    let mut pins = Vec::new();
    for name in CONFIGS {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        let table = read_table(&path)?;
        if let Some(entry) = tools_entry(&table, tool) {
            pins.push(requested(entry, tool, name)?);
        }
    }
    let version = match pins.as_slice() {
        [] => {
            return Err(CiError::new(format!(
                "{tool} is not pinned in {} or .mise.toml",
                CONFIGS[0]
            )));
        }
        [first, rest @ ..] if rest.iter().all(|other| other == first) => first.clone(),
        _ => {
            return Err(CiError::new(format!(
                "{tool} has different pins in mise.toml and .mise.toml"
            )));
        }
    };
    check_lock(dir, tool, &version)?;
    Ok(version)
}

pub fn append_output(path: &Path, version: &str) -> Result<(), CiError> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "version={version}"))
        .map_err(|error| CiError::new(format!("cannot write {}: {error}", path.display())))
}

pub fn run(tool: &str) -> Result<String, CiError> {
    let cwd = std::env::current_dir()
        .map_err(|error| CiError::new(format!("cannot read the working directory: {error}")))?;
    let version = pinned_version(&cwd, tool)?;
    if let Some(output) = std::env::var_os(OUTPUT_VARIABLE).filter(|path| !path.is_empty()) {
        append_output(Path::new(&output), &version)?;
    }
    Ok(version)
}

fn read_table(path: &Path) -> Result<Table, CiError> {
    let text = fs::read_to_string(path)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", path.display())))?;
    text.parse::<Table>()
        .map_err(|error| CiError::new(format!("{} does not parse: {error}", path.display())))
}

fn tools_entry<'a>(table: &'a Table, tool: &str) -> Option<&'a Value> {
    table.get("tools")?.as_table()?.get(tool)
}

fn requested(entry: &Value, tool: &str, file: &str) -> Result<String, CiError> {
    let text = match entry {
        Value::String(text) => text.as_str(),
        Value::Table(table) => table
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| not_exact(tool, file, "no string version"))?,
        _ => return Err(not_exact(tool, file, "not a single version")),
    };
    velnor_rust::exact_release(text)
        .map(str::to_owned)
        .ok_or_else(|| not_exact(tool, file, text))
}

fn not_exact(tool: &str, file: &str, found: &str) -> CiError {
    CiError::new(format!(
        "{tool} in {file} must pin one exact X.Y.Z release, found {found:?}"
    ))
}

fn check_lock(dir: &Path, tool: &str, version: &str) -> Result<(), CiError> {
    let path = dir.join(LOCK);
    if !path.exists() {
        return Ok(());
    }
    let lock = read_table(&path)?;
    let rows = lock
        .get("tools")
        .and_then(Value::as_table)
        .and_then(|tools| tools.get(tool))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .ok_or_else(|| CiError::new(format!("mise.lock has no row for {tool}")))?;
    for row in rows {
        let locked = row.as_table().and_then(|row| row.get("version"));
        if locked.and_then(Value::as_str) != Some(version) {
            return Err(CiError::new(format!(
                "mise.lock resolves {tool} to {locked:?}, not the pinned {version}"
            )));
        }
    }
    Ok(())
}
