// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::error::CiError;

pub type TestKey = (String, String);

#[derive(Debug, Deserialize)]
pub struct Listing {
    #[serde(rename = "rust-suites")]
    suites: BTreeMap<String, Suite>,
}

#[derive(Debug, Deserialize)]
struct Suite {
    #[serde(default)]
    testcases: BTreeMap<String, Case>,
}

#[derive(Debug, Deserialize)]
pub struct Case {
    #[serde(default)]
    pub ignored: bool,
    #[serde(rename = "filter-match")]
    filter_match: FilterMatch,
}

#[derive(Debug, Deserialize)]
struct FilterMatch {
    status: String,
}

impl Listing {
    fn parse(prefix: &str, bytes: &[u8]) -> Result<Self, CiError> {
        serde_json::from_slice(bytes).map_err(|e| {
            CiError::new(format!(
                "{prefix}: cargo-nextest list returned invalid JSON: {e}"
            ))
        })
    }

    pub fn cases(&self) -> impl Iterator<Item = (TestKey, &Case)> {
        self.suites.iter().flat_map(|(binary, suite)| {
            suite
                .testcases
                .iter()
                .map(move |(name, case)| ((binary.clone(), name.clone()), case))
        })
    }

    #[must_use]
    pub fn keys(&self) -> BTreeSet<TestKey> {
        self.cases().map(|(key, _)| key).collect()
    }

    #[must_use]
    pub fn selected(&self) -> BTreeSet<TestKey> {
        self.cases()
            .filter(|(_, case)| case.filter_match.status == "matches")
            .map(|(key, _)| key)
            .collect()
    }
}

pub fn list(prefix: &str, extra: &[String]) -> Result<Listing, CiError> {
    let output = Command::new("cargo-nextest")
        .args([
            "nextest",
            "list",
            "--workspace",
            "--locked",
            "--ignore-default-filter",
            "--message-format",
            "json",
        ])
        .args(extra)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| CiError::new(format!("{prefix}: cannot run cargo-nextest: {e}")))?;
    if !output.status.success() {
        let exit = output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| format!("exit {code}"));
        return Err(CiError::new(format!(
            "{prefix}: cargo-nextest list failed ({exit})"
        )));
    }
    Listing::parse(prefix, &output.stdout)
}
