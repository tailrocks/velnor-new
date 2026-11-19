// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;

use crate::cli::LaneGuardArgs;
use crate::error::CiError;
use crate::escape::escape;

const PREFIX: &str = "velnor-ci lane-guard";
const LISTED_LIMIT: usize = 50;

type TestKey = (String, String);

#[derive(Debug, Deserialize)]
struct Listing {
    #[serde(rename = "rust-suites")]
    suites: BTreeMap<String, Suite>,
}

#[derive(Debug, Deserialize)]
struct Suite {
    #[serde(default)]
    testcases: BTreeMap<String, Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    #[serde(default)]
    ignored: bool,
    #[serde(rename = "filter-match")]
    filter_match: FilterMatch,
}

#[derive(Debug, Deserialize)]
struct FilterMatch {
    status: String,
}

impl Listing {
    fn parse(bytes: &[u8]) -> Result<Self, CiError> {
        serde_json::from_slice(bytes).map_err(|e| {
            CiError::new(format!(
                "{PREFIX}: cargo-nextest list returned invalid JSON: {e}"
            ))
        })
    }

    fn cases(&self) -> impl Iterator<Item = (TestKey, &Case)> {
        self.suites.iter().flat_map(|(binary, suite)| {
            suite
                .testcases
                .iter()
                .map(move |(name, case)| ((binary.clone(), name.clone()), case))
        })
    }

    fn keys(&self) -> BTreeSet<TestKey> {
        self.cases().map(|(key, _)| key).collect()
    }

    fn selected(&self) -> BTreeSet<TestKey> {
        self.cases()
            .filter(|(_, case)| case.filter_match.status == "matches")
            .map(|(key, _)| key)
            .collect()
    }
}

#[must_use]
pub fn run(args: &LaneGuardArgs) -> ExitCode {
    match guard(args) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn guard(args: &LaneGuardArgs) -> Result<String, CiError> {
    let excluded: String = args
        .except_binary
        .iter()
        .map(|binary| [" - binary_id(=", binary, ")"].concat())
        .collect();
    let default_filter: Vec<String> = if excluded.is_empty() {
        Vec::new()
    } else {
        vec!["-E".to_owned(), format!("all(){excluded}")]
    };
    let features = ["--features".to_owned(), args.features.clone()];
    let selection = ["-E".to_owned(), format!("({}){excluded}", args.filter)];
    let default = list(&default_filter)?;
    let universe = list(&[default_filter.as_slice(), &features].concat())?;
    let selected = list(&[features.as_slice(), &selection].concat())?;
    let default_keys = default.keys();
    let selected_keys = selected.selected();
    let feature_only: Vec<(TestKey, &Case)> = universe
        .cases()
        .filter(|(key, case)| !default_keys.contains(key) && !case.ignored)
        .collect();
    let missing: Vec<TestKey> = feature_only
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !selected_keys.contains(key))
        .collect();
    if missing.is_empty() {
        return Ok(success(&args.features, feature_only.len()));
    }
    Err(CiError::new(failure(args, &missing)))
}

fn list(extra: &[String]) -> Result<Listing, CiError> {
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
        .map_err(|e| CiError::new(format!("{PREFIX}: cannot run cargo-nextest: {e}")))?;
    if !output.status.success() {
        let exit = output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| format!("exit {code}"));
        return Err(CiError::new(format!(
            "{PREFIX}: cargo-nextest list failed ({exit})"
        )));
    }
    Listing::parse(&output.stdout)
}

fn success(features: &str, checked: usize) -> String {
    let features = escape(features, false);
    match checked {
        0 => format!(
            "{PREFIX}: no runnable tests exist only with features '{features}'; nothing to check\n"
        ),
        1 => format!(
            "{PREFIX}: the 1 test that exists only with features '{features}' is selected by the lane filter\n"
        ),
        n => format!(
            "{PREFIX}: all {n} tests that exist only with features '{features}' are selected by the lane filter\n"
        ),
    }
}

fn failure(args: &LaneGuardArgs, missing: &[TestKey]) -> String {
    let (count, verb, falls, them) = match missing.len() {
        1 => ("1 test".to_owned(), "exists", "falls", "it"),
        n => (format!("{n} tests"), "exist", "fall", "them"),
    };
    let mut lines = vec![
        format!(
            "{PREFIX}: {count} {verb} only with features '{}' and {falls} outside the lane filter",
            escape(&args.features, false)
        ),
        format!("  filter: {}", escape(&args.filter, false)),
    ];
    for (binary, name) in missing.iter().take(LISTED_LIMIT) {
        lines.push(format!(
            "  not selected: {} {}",
            escape(binary, false),
            escape(name, false)
        ));
    }
    if missing.len() > LISTED_LIMIT {
        lines.push(format!("  ... and {} more", missing.len() - LISTED_LIMIT));
    }
    lines.push(format!(
        "extend the filter of the [[test_lane]] in .velnor/rust-ci.toml so it selects {them}, then run velnor-gen generate"
    ));
    lines.join("\n")
}
