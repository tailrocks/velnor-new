// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::waste_evidence::MbxSession;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::Path;

pub const SWITCH: &str = "VELNOR_MBX_REPORTS";
pub const DIRECTORY: &str = "velnor-mbx-reports";
pub const VARIABLE: &str = "MBX_STATS_REPORT";
pub const KNOWN_VERSIONS: &[u64] = &[5];
const ROUTINE_PROBES: &[&str] = &[
    "compiler-query",
    "standard-input",
    "cc-compiler-query",
    "cc-standard-input",
];
const BUILD_ONLY_SUBCOMMANDS: &[&str] =
    &["build", "b", "check", "c", "clippy", "doc", "d", "rustc"];

#[derive(Deserialize)]
struct Counts {
    hits: u64,
    misses: u64,
    unconsulted: u64,
    #[serde(default)]
    bypasses: Map<String, Value>,
}

const VALUE_OPTIONS: &[&str] = &["-C", "-Z", "--config", "--color"];
const VALUE_FREE_OPTIONS: &[&str] = &[
    "-q",
    "--quiet",
    "--verbose",
    "--frozen",
    "--locked",
    "--offline",
];

fn is_complete_option(word: &str) -> bool {
    VALUE_FREE_OPTIONS.contains(&word)
        || word
            .strip_prefix('-')
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte == b'v'))
        || (word.len() > 2 && (word.starts_with("-C") || word.starts_with("-Z")))
        || word.starts_with("--config=")
        || word.starts_with("--color=")
}

fn cargo_subcommand(words: &[String]) -> Option<&str> {
    let mut words = words.iter().map(String::as_str).peekable();
    if words.peek().is_some_and(|word| word.starts_with('+')) {
        words.next();
    }
    while let Some(word) = words.next() {
        if VALUE_OPTIONS.contains(&word) {
            words.next()?;
        } else if !is_complete_option(word) {
            return (!word.starts_with('-')).then_some(word);
        }
    }
    None
}

#[must_use]
pub fn reports_session(argv: &[String]) -> bool {
    let Some(program) = argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let subcommand = if program == "cargo" {
        cargo_subcommand(&argv[1..])
    } else {
        program.strip_prefix("cargo-")
    };
    subcommand.is_some_and(|name| BUILD_ONLY_SUBCOMMANDS.contains(&name))
}

/// # Errors
///
/// The report is not JSON, has an unknown `version`, or lacks a count.
pub fn parse(report: &[u8]) -> Result<Option<MbxSession>, String> {
    let value: Value =
        serde_json::from_slice(report).map_err(|error| format!("not JSON ({error})"))?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "no numeric version".to_owned())?;
    if !KNOWN_VERSIONS.contains(&version) {
        return Err(format!(
            "version {version}, while this reader knows {KNOWN_VERSIONS:?}"
        ));
    }
    let counts: Counts = serde_json::from_value(value)
        .map_err(|error| format!("version {version} without its counts ({error})"))?;
    let bypassed = counts
        .bypasses
        .keys()
        .any(|reason| !ROUTINE_PROBES.contains(&reason.as_str()));
    let looked_at = counts.hits > 0 || counts.misses > 0 || counts.unconsulted > 0;
    Ok((looked_at || bypassed).then_some(MbxSession {
        hits: counts.hits,
        misses: counts.misses,
        not_looked_up: counts.unconsulted,
    }))
}
