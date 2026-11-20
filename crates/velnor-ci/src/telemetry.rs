// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INVOCATIONS: &str = "velnor-invocations.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub argv: Vec<String>,
    pub root: String,
    pub configuration: String,
    pub started_at: u64,
    pub seconds: f64,
    pub log_start: usize,
    pub log_end: usize,
    pub compilations: Vec<Compilation>,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub verified_download_archives: Option<Vec<String>>,
    #[serde(default)]
    pub verified_git_downloads: Option<Vec<String>>,
    #[serde(default)]
    pub restored_target_usable: Option<bool>,
    #[serde(default)]
    pub artifacts: Vec<crate::cargo_artifacts::CargoArtifact>,
    #[serde(default)]
    pub build_scripts: Vec<crate::cargo_artifacts::BuildScriptRun>,
    #[serde(default)]
    pub progress: Option<Vec<crate::cargo_artifacts::CargoProgress>>,
    #[serde(default)]
    pub capture_complete: bool,
    #[serde(default)]
    pub tests: Vec<(String, f64)>,
    #[serde(default)]
    pub rejected: u64,
    #[serde(default)]
    pub execution: BTreeMap<String, String>,
    #[serde(default)]
    pub target_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compilation {
    pub package: String,
    pub version: String,
    pub workspace: bool,
    pub unchanged: bool,
    pub dependencies: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<bool>,
}

#[must_use]
pub fn configuration(argv: &[String]) -> String {
    let mut options = BTreeMap::new();
    let mut features = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut configs = Vec::new();
    let mut words = argv.iter().take_while(|word| word.as_str() != "--");
    while let Some(word) = words.next() {
        let (flag, inline) = word
            .split_once('=')
            .map_or((word.as_str(), None), |(flag, value)| (flag, Some(value)));
        let (flag, inline) = flag
            .strip_prefix("-F")
            .filter(|value| !value.is_empty())
            .map_or((flag, inline), |value| ("-F", Some(value)));
        match flag {
            "--features" | "-F" | "--target" | "--profile" | "--config" => {
                let Some(value) = inline.or_else(|| words.next().map(String::as_str)) else {
                    continue;
                };
                match flag {
                    "--features" | "-F" => features.extend(
                        value
                            .split(|character: char| character == ',' || character.is_whitespace())
                            .filter(|feature| !feature.is_empty()),
                    ),
                    "--target" => {
                        targets.insert(value);
                    }
                    "--config" => configs.push(value),
                    _ => {
                        options.insert(flag, value);
                    }
                }
            }
            "--release" | "-r" => {
                options.insert("--profile", "release");
            }
            "--all-features" | "--no-default-features" => {
                options.insert(flag, "");
            }
            _ => {}
        }
    }
    format!("{options:?} {features:?} {targets:?} {configs:?}")
}

#[must_use]
pub fn target_selection(argv: &[String]) -> BTreeSet<String> {
    let mut selection = BTreeSet::new();
    let mut words = argv.iter().skip(1).take_while(|word| word.as_str() != "--");
    let mut command = None;
    let mut nextest_mode = false;
    while let Some(word) = words.next() {
        let (flag, inline) = word
            .split_once('=')
            .map_or((word.as_str(), None), |(flag, value)| (flag, Some(value)));
        match flag {
            "--bin" | "--test" | "--example" | "--bench" => {
                if let Some(value) = inline.or_else(|| words.next().map(String::as_str)) {
                    selection.insert(format!("{flag}={value}"));
                }
            }
            "--lib" | "--bins" | "--tests" | "--examples" | "--benches" | "--all-targets"
            | "--doc" => {
                selection.insert(flag.to_owned());
            }
            "--config" | "--color" | "--manifest-path" | "--lockfile-path" | "--target-dir"
            | "--target" | "--profile" | "--features" | "--package" | "--exclude" | "--jobs"
            | "--message-format" | "-C" | "-Z" | "-F" | "-p" | "-j" => {
                if inline.is_none() {
                    words.next();
                }
            }
            _ if word.starts_with('+') && command.is_none() => {}
            _ if !word.starts_with('-') && command.is_none() => {
                command = Some(word.as_str());
                selection.insert(word.clone());
            }
            _ if command == Some("nextest") && !nextest_mode && !word.starts_with('-') => {
                selection.insert(format!("nextest:{word}"));
                nextest_mode = true;
            }
            _ => {}
        }
    }
    selection
}
