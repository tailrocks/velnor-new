// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::setup::{check_tools, parse_reported};
use velnor_model::{ToolPin, ToolPins};

type Case<'a> = (&'a [&'a str], &'a [&'a str], &'a [(&'a str, bool)], bool);

fn pins(requested: &[&str], locked: &[&str]) -> ToolPins {
    ToolPins {
        configs: Vec::new(),
        tools: BTreeMap::from([(
            "tool".to_owned(),
            ToolPin {
                requested: requested.iter().map(|v| (*v).to_owned()).collect(),
                locked: locked.iter().map(|v| (*v).to_owned()).collect(),
            },
        )]),
    }
}

fn reported(versions: &[(&str, bool)]) -> String {
    let entries: Vec<String> = versions
        .iter()
        .map(|(version, installed)| {
            format!("{{\"version\":\"{version}\",\"installed\":{installed}}}")
        })
        .collect();
    format!("{{\"tool\":[{}]}}", entries.join(","))
}

#[test]
fn a_tool_passes_only_when_an_installed_version_equals_the_lock_or_the_pin() {
    let cases: [Case<'_>; 10] = [
        (&["1.2.3"], &[], &[("1.2.3", true)], true),
        (&["1.2.3"], &[], &[("1.2.4", true)], false),
        (&["1.2"], &[], &[("1.2.9", true)], true),
        (&["1.2"], &[], &[("1.20.0", true)], false),
        (&["1"], &["1.2.3"], &[("1.9.0", true)], false),
        (&["1"], &["1.2.3"], &[("1.2.3", true)], true),
        (&["1.2.3"], &[], &[("1.2.3", false)], false),
        (&["1.2.3"], &[], &[], false),
        (&["1.2"], &["1.2.3"], &[("1.2.3.1", true)], false),
        (&["1.2"], &[], &[("1.2.3.1", true)], true),
    ];
    for (requested, locked, installed, ok) in cases {
        let json = reported(installed);
        let result = check_tools(
            &pins(requested, locked),
            &parse_reported(&json).unwrap_or_else(|e| unreachable!("{e}")),
        );
        assert_eq!(result.is_ok(), ok, "{requested:?} {locked:?} {installed:?}");
    }
}

#[test]
fn an_alias_passes_when_mise_reports_it_as_the_request_it_resolved() {
    let json =
        "{\"tool\":[{\"version\":\"24.11.1\",\"requested_version\":\"lts\",\"installed\":true}]}";
    let reported = parse_reported(json).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check_tools(&pins(&["lts"], &[]), &reported), Ok(()));
    assert!(check_tools(&pins(&["latest-lts"], &[]), &reported).is_err());
    assert!(check_tools(&pins(&["lts"], &["24.11.0"]), &reported).is_err());
}

#[test]
fn every_requested_version_must_be_installed() {
    let reported = parse_reported(&reported(&[("20.1.0", true), ("22.3.0", false)]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let error = check_tools(&pins(&["20", "22"], &[]), &reported)
        .err()
        .unwrap_or_else(|| unreachable!("a missing version must fail"));
    assert!(error.to_string().contains("22"), "{error}");
    assert!(!error.to_string().contains("pinned 20"), "{error}");
    let both = parse_reported(&reported_both()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check_tools(&pins(&["20", "22"], &[]), &both), Ok(()));
}

fn reported_both() -> String {
    reported(&[("20.1.0", true), ("22.3.0", true)])
}
