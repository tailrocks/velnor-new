// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::waste_evidence::{MbxCompilerWork, MbxSession};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fs;
use std::os::unix::fs::FileTypeExt;
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
    verifications: u64,
    #[serde(default)]
    bypasses: Map<String, Value>,
    #[serde(default)]
    compiler: Map<String, Value>,
}

#[must_use]
pub fn is_socket(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}

const VALUE_LONG_OPTIONS: &[&str] = &["--config", "--color"];
const VALUE_FREE_LONG_OPTIONS: &[&str] =
    &["--quiet", "--verbose", "--frozen", "--locked", "--offline"];

/// Whether a cargo global option takes the next word, or `None` when it is not one of these.
fn global_option(word: &str) -> Option<bool> {
    if let Some(shorts) = word
        .strip_prefix('-')
        .filter(|rest| !rest.is_empty() && !rest.starts_with('-'))
    {
        return short_cluster(shorts);
    }
    if VALUE_LONG_OPTIONS.contains(&word) {
        return Some(true);
    }
    (VALUE_FREE_LONG_OPTIONS.contains(&word)
        || word.starts_with("--config=")
        || word.starts_with("--color="))
    .then_some(false)
}

/// `-qv` and `-vC dir` are bundles: `C` and `Z` take the rest of the bundle, or the next word when none is left.
fn short_cluster(shorts: &str) -> Option<bool> {
    for (index, letter) in shorts.char_indices() {
        match letter {
            'v' | 'q' => {}
            'C' | 'Z' => return Some(index + letter.len_utf8() == shorts.len()),
            _ => return None,
        }
    }
    Some(false)
}

fn cargo_subcommand(words: &[String]) -> Option<(&str, &[String])> {
    let mut index = usize::from(words.first().is_some_and(|word| word.starts_with('+')));
    while let Some(word) = words.get(index) {
        index += 1;
        match global_option(word) {
            Some(takes_next) => {
                if takes_next {
                    words.get(index)?;
                    index += 1;
                }
            }
            None => {
                return (!word.starts_with('-')).then(|| (word.as_str(), &words[index..]));
            }
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
    if program == "cargo" {
        return cargo_subcommand(&argv[1..]).is_some_and(|(name, after)| {
            BUILD_ONLY_SUBCOMMANDS.contains(&name) || (name == "nextest" && lists(after))
        });
    }
    match program.strip_prefix("cargo-") {
        Some("nextest") => lists(argv.get(2..).unwrap_or_default()),
        Some(name) => BUILD_ONLY_SUBCOMMANDS.contains(&name),
        None => false,
    }
}

fn lists(words: &[String]) -> bool {
    words.first().is_some_and(|word| word == "list")
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
    let looked_at = counts.hits > 0 || counts.misses > 0 || counts.unconsulted > 0;
    let bypassed = !counts.bypasses.is_empty();
    let probe_only = !looked_at
        && counts.verifications == 0
        && counts
            .bypasses
            .keys()
            .all(|reason| ROUTINE_PROBES.contains(&reason.as_str()));
    Ok(
        (looked_at || bypassed || counts.verifications > 0).then_some(MbxSession {
            hits: counts.hits,
            misses: counts.misses,
            not_looked_up: counts.unconsulted,
            verifications: Some(counts.verifications).filter(|count| *count > 0),
            bypasses: counts
                .bypasses
                .iter()
                .filter_map(|(reason, count)| Some((reason.clone(), count.as_u64()?)))
                .collect(),
            compiler: counts
                .compiler
                .into_iter()
                .filter_map(|(outcome, work)| {
                    Some((
                        outcome,
                        serde_json::from_value::<MbxCompilerWork>(work).ok()?,
                    ))
                })
                .collect(),
            probe_only,
        }),
    )
}
