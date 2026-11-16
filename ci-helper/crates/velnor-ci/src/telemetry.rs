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
    pub artifacts: Vec<crate::cargo_artifacts::CargoArtifact>,
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
}

#[must_use]
pub fn configuration(argv: &[String]) -> String {
    let mut options = BTreeMap::new();
    let mut features = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut configs = Vec::new();
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let (flag, inline) = word
            .split_once('=')
            .map_or((word.as_str(), None), |(flag, value)| (flag, Some(value)));
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
            "--release" => {
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
