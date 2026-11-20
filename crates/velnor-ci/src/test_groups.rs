// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use crate::cli::{GroupFilter, TestGroupsCheckArgs};
use crate::error::CiError;
use crate::escape::escape;
use crate::nextest_list::{TestKey, list};

const PREFIX: &str = "velnor-ci test-groups";
const LISTED_LIMIT: usize = 50;

pub fn run(args: &TestGroupsCheckArgs) -> ExitCode {
    match check(&args.groups) {
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

fn check(groups: &[GroupFilter]) -> Result<String, CiError> {
    let mut names = BTreeSet::new();
    for group in groups {
        if !names.insert(group.name.as_str()) {
            return Err(CiError::new(format!(
                "{PREFIX}: the group {} is named twice",
                escape(&group.name, false)
            )));
        }
    }
    let mut owners: BTreeMap<TestKey, Vec<&str>> = BTreeMap::new();
    for group in groups {
        let listing = list(PREFIX, &["-E".to_owned(), group.filter.clone()])?;
        let selected = listing.selected();
        for (key, case) in listing.cases() {
            if !case.ignored && selected.contains(&key) {
                owners.entry(key).or_default().push(group.name.as_str());
            }
        }
    }
    let shared: Vec<(&TestKey, &Vec<&str>)> = owners
        .iter()
        .filter(|(_, groups)| groups.len() > 1)
        .collect();
    if shared.is_empty() {
        return Ok(success(groups.len(), owners.len()));
    }
    Err(CiError::new(failure(&shared)))
}

fn plural(count: usize, one: &'static str, many: &'static str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn success(groups: usize, tests: usize) -> String {
    format!(
        "{PREFIX}: {} {} {} and no test is in two groups\n",
        groups,
        if groups == 1 {
            "group selects"
        } else {
            "groups select"
        },
        plural(tests, "test", "tests"),
    )
}

fn names(groups: &[&str]) -> String {
    let shown: Vec<String> = groups.iter().map(|name| escape(name, false)).collect();
    match shown.split_last() {
        Some((last, first)) if !first.is_empty() => format!("{} and {last}", first.join(", ")),
        _ => shown.concat(),
    }
}

fn failure(shared: &[(&TestKey, &Vec<&str>)]) -> String {
    let count = if shared.len() == 1 {
        "1 test matches".to_owned()
    } else {
        format!("{} tests match", shared.len())
    };
    let mut lines = vec![format!("{PREFIX}: {count} more than one group")];
    for ((binary, name), groups) in shared.iter().take(LISTED_LIMIT) {
        lines.push(format!(
            "  {} {} matches {}",
            escape(binary, false),
            escape(name, false),
            names(groups)
        ));
    }
    if shared.len() > LISTED_LIMIT {
        lines.push(format!("  ... and {} more", shared.len() - LISTED_LIMIT));
    }
    lines.push(
        "give each test to one group: narrow the filter of one of them in [[test_groups]] in .velnor/rust-ci.toml, then run velnor-gen generate"
            .to_owned(),
    );
    lines.join("\n")
}
