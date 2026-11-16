// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::setup::{Reported, check_tools, parse_reported};
use velnor_model::{ToolPin, ToolPins};

type Case<'a> = (&'a [&'a str], &'a [&'a str], &'a [(&'a str, bool)], bool);

fn pins(requested: &[&str], locked: &[&str]) -> ToolPins {
    ToolPins {
        configs: Vec::new(),
        locked_file: true,
        tools: BTreeMap::from([(
            "tool".to_owned(),
            ToolPin {
                requested: requested.iter().map(|v| (*v).to_owned()).collect(),
                locked: locked.iter().map(|v| (*v).to_owned()).collect(),
                os_filters: BTreeMap::new(),
                ..ToolPin::default()
            },
        )]),
    }
}

fn check(pins: &ToolPins, reported: &Reported) -> Result<(), CiError> {
    check_tools(pins, reported, "linux")
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
        let result = check(
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
    assert_eq!(check(&pins(&["lts"], &[]), &reported), Ok(()));
    assert!(check(&pins(&["latest-lts"], &[]), &reported).is_err());
    assert!(check(&pins(&["lts"], &["24.11.0"]), &reported).is_err());
}

#[test]
fn every_requested_version_must_be_installed() {
    let reported = parse_reported(&reported(&[("20.1.0", true), ("22.3.0", false)]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let error = check(&pins(&["20", "22"], &[]), &reported)
        .err()
        .unwrap_or_else(|| unreachable!("a missing version must fail"));
    assert!(error.to_string().contains("22"), "{error}");
    assert!(!error.to_string().contains("pinned 20"), "{error}");
    let both = parse_reported(&reported_both()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check(&pins(&["20", "22"], &[]), &both), Ok(()));
}

fn reported_both() -> String {
    reported(&[("20.1.0", true), ("22.3.0", true)])
}

fn filtered(requested: &[(&str, &[&str])], locked: &[&str]) -> ToolPins {
    let mut pins = pins(
        &requested.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        locked,
    );
    if let Some(pin) = pins.tools.get_mut("tool") {
        for (version, filters) in requested.iter().filter(|(_, f)| !f.is_empty()) {
            pin.os_filters.insert(
                (*version).to_owned(),
                filters.iter().map(|f| (*f).to_owned()).collect(),
            );
        }
    }
    pins
}

#[test]
fn a_request_filtered_to_another_os_is_not_required_here() {
    let both = [("1.22", &["macos"][..]), ("1.23", &["linux"][..])];
    let linux =
        parse_reported(&reported(&[("1.23.4", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    let mac =
        parse_reported(&reported(&[("1.22.9", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check_tools(&filtered(&both, &[]), &linux, "linux"), Ok(()));
    assert_eq!(check_tools(&filtered(&both, &[]), &mac, "macos"), Ok(()));
    assert_eq!(
        check_tools(&filtered(&both, &["1.22.9", "1.23.4"]), &linux, "linux"),
        Ok(())
    );
    let error = check_tools(&filtered(&both, &[]), &mac, "linux")
        .err()
        .unwrap_or_else(|| unreachable!("the applicable version is missing"));
    assert!(error.to_string().contains("1.23"), "{error}");
}

#[test]
fn overlapping_requests_each_need_their_own_installed_entry() {
    let narrow =
        "{\"tool\":[{\"version\":\"1.22.12\",\"requested_version\":\"1.22\",\"installed\":true}]}";
    let wide = "{\"tool\":[{\"version\":\"1.22.12\",\"requested_version\":\"1.22\",\"installed\":true},{\"version\":\"1.26.7\",\"requested_version\":\"1\",\"installed\":true}]}";
    let narrow = parse_reported(narrow).unwrap_or_else(|e| unreachable!("{e}"));
    let wide = parse_reported(wide).unwrap_or_else(|e| unreachable!("{e}"));
    let error = check(&pins(&["1", "1.22"], &[]), &narrow)
        .err()
        .unwrap_or_else(|| unreachable!("request 1 has no entry"));
    assert!(error.to_string().contains("pinned 1,"), "{error}");
    assert_eq!(check(&pins(&["1", "1.22"], &[]), &wide), Ok(()));
}
