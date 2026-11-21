// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use crate::cli::LaneGuardArgs;
use crate::error::CiError;
use crate::escape::escape;
use crate::nextest_list::{Case, TestKey, list};

const PREFIX: &str = "velnor-ci lane-guard";
const LISTED_LIMIT: usize = 50;

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
    let features = feature_args(args);
    let selection = ["-E".to_owned(), format!("({}){excluded}", args.filter)];
    let default = list(PREFIX, &default_filter)?;
    let universe = list(PREFIX, &[default_filter.as_slice(), &features].concat())?;
    let selected = list(PREFIX, &[features.as_slice(), &selection].concat())?;
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
        return Ok(success(&describe(args), feature_only.len()));
    }
    Err(CiError::new(failure(args, &missing)))
}

fn feature_args(args: &LaneGuardArgs) -> Vec<String> {
    let mut flags = Vec::new();
    if args.all_features {
        flags.push("--all-features".to_owned());
    }
    if args.no_default_features {
        flags.push("--no-default-features".to_owned());
    }
    if let Some(features) = &args.features {
        flags.push("--features".to_owned());
        flags.push(features.clone());
    }
    flags
}

fn describe(args: &LaneGuardArgs) -> String {
    let names = args
        .features
        .as_deref()
        .map(|features| format!("features '{}'", escape(features, false)));
    match (args.all_features, args.no_default_features, names) {
        (true, _, _) => "all features".to_owned(),
        (false, true, None) => "no default features".to_owned(),
        (false, true, Some(names)) => format!("no default features and {names}"),
        (false, false, Some(names)) => names,
        (false, false, None) => "no features".to_owned(),
    }
}

fn success(features: &str, checked: usize) -> String {
    match checked {
        0 => format!("{PREFIX}: no runnable tests exist only with {features}; nothing to check\n"),
        1 => format!(
            "{PREFIX}: the 1 test that exists only with {features} is selected by the lane filter\n"
        ),
        n => format!(
            "{PREFIX}: all {n} tests that exist only with {features} are selected by the lane filter\n"
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
            "{PREFIX}: {count} {verb} only with {} and {falls} outside the lane filter",
            describe(args)
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
