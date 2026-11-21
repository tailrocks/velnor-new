// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::crates_mirror_mbx::{direct_target, option_value, subcommand};

const DEPTH: usize = 8;
const CONFIGS: &[&str] = &["config", "config.toml"];
const ALIASABLE_BUILTINS: &[&str] = &["b", "c", "clippy", "d", "r", "rm", "t"];

enum Alias {
    Found(Vec<OsString>),
    Absent,
    Unknown,
}

pub(super) fn expand(work: &Path, args: &[OsString], builtin: &[&str]) -> Option<Vec<OsString>> {
    let mut args = args.to_vec();
    for _ in 0..DEPTH {
        let Some((index, name)) = subcommand(&args) else {
            return Some(args);
        };
        if name == "install" {
            return Some(args);
        }
        let name = name.to_owned();
        if builtin.contains(&name.as_str()) && !ALIASABLE_BUILTINS.contains(&name.as_str()) {
            return Some(args);
        }
        if direct_target(&name) {
            return matches!(lookup(work, &args, &name), Alias::Absent).then_some(args);
        }
        match lookup(work, &args, &name) {
            Alias::Found(words) => {
                args.splice(index..=index, words);
            }
            Alias::Absent => return Some(args),
            Alias::Unknown if builtin.contains(&name.as_str()) => return None,
            Alias::Unknown => {
                return option_value(&args, "--manifest-path", true)
                    .is_none()
                    .then_some(args);
            }
        }
    }
    None
}

fn lookup(work: &Path, args: &[OsString], name: &str) -> Alias {
    if command_line_may_define_aliases(args) {
        return Alias::Unknown;
    }
    let variable = format!(
        "CARGO_ALIAS_{}",
        name.to_uppercase().replace(['-', '.'], "_")
    );
    if let Some(value) = std::env::var_os(&variable) {
        return match value.to_str() {
            Some(text) => words(text),
            None => Alias::Unknown,
        };
    }
    for directory in config_directories(work) {
        match config_text(&directory) {
            Ok(Some(text)) => match from_config(&text, name) {
                Alias::Absent => {}
                found => return found,
            },
            Ok(None) => {}
            Err(_) => return Alias::Unknown,
        }
    }
    Alias::Absent
}

fn config_text(directory: &Path) -> std::io::Result<Option<String>> {
    for config in CONFIGS {
        match std::fs::read_to_string(directory.join(config)) {
            Ok(text) => return Ok(Some(text)),
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn command_line_may_define_aliases(args: &[OsString]) -> bool {
    let mut args = args.iter().take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let value = args.next().and_then(|value| value.to_str());
            if value.is_none_or(|value| value.starts_with("alias") || !value.contains('=')) {
                return true;
            }
        } else if arg
            .to_str()
            .and_then(|arg| arg.strip_prefix("--config="))
            .is_some_and(|value| value.starts_with("alias") || !value.contains('='))
        {
            return true;
        }
    }
    false
}

fn config_directories(work: &Path) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = work
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .collect();
    let home = std::env::var_os("CARGO_HOME")
        .filter(|value| !value.is_empty())
        .map(|value| work.join(value))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    directories.extend(home);
    directories
}

fn from_config(text: &str, name: &str) -> Alias {
    let Ok(table) = text.parse::<Table>() else {
        return Alias::Unknown;
    };
    if table.contains_key("include") {
        return Alias::Unknown;
    }
    match table.get("alias") {
        None => Alias::Absent,
        Some(Value::Table(aliases)) => match aliases.get(name) {
            None => Alias::Absent,
            Some(Value::String(text)) => words(text),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(OsString::from))
                .collect::<Option<Vec<_>>>()
                .filter(|words| !words.is_empty())
                .map_or(Alias::Unknown, Alias::Found),
            Some(_) => Alias::Unknown,
        },
        Some(_) => Alias::Unknown,
    }
}

fn words(text: &str) -> Alias {
    let words: Vec<OsString> = text.split_whitespace().map(OsString::from).collect();
    if words.is_empty() {
        Alias::Unknown
    } else {
        Alias::Found(words)
    }
}
