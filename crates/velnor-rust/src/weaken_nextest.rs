// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Table;
use toml::Value;
use toml::map::Map;

use crate::weaken::Make;
use velnor_model::Weakening;

const CHECKED_PROFILES: &[&str] = &["default", "ci"];

fn profiles(parsed: &Table) -> impl Iterator<Item = (&String, &Value)> {
    parsed
        .get("profile")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
}

fn checked(parsed: &Table) -> impl Iterator<Item = (&String, &Value)> {
    profiles(parsed).filter(|(profile, _)| CHECKED_PROFILES.contains(&profile.as_str()))
}

fn entries<'a>(section: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    section
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn retries_tests(value: &Value) -> bool {
    value.get("count").unwrap_or(value).as_integer() != Some(0)
}

fn flaky_result(section: &Value) -> String {
    section
        .get("flaky-result")
        .map_or_else(String::new, |result| format!(" flaky-result = {result}"))
}

fn passes_on_timeout(value: &Value) -> bool {
    value.get("on-timeout").and_then(Value::as_str) == Some("pass")
}

pub(crate) fn nextest(parsed: &Table, make: &Make<'_>) -> Vec<Weakening> {
    let commands: Option<&Map<String, Value>> = parsed
        .get("scripts")
        .and_then(|scripts| scripts.get("wrapper"))
        .and_then(Value::as_table);
    let mut found = Vec::new();
    for (profile, section) in checked(parsed) {
        for entry in entries(section, "scripts") {
            for key in ["run-wrapper", "list-wrapper"] {
                let Some(name) = entry.get(key).and_then(Value::as_str) else {
                    continue;
                };
                let command = commands
                    .and_then(|commands| commands.get(name))
                    .and_then(|wrapper| wrapper.get("command"))
                    .map_or_else(|| "an unnamed command".to_owned(), ToString::to_string);
                let filter = entry
                    .get("filter")
                    .map_or_else(String::new, |filter| format!(" filter = {filter}"));
                let platform = entry
                    .get("platform")
                    .map_or_else(String::new, |platform| format!(" platform = {platform}"));
                let stage = if key == "run-wrapper" {
                    "runs test binaries"
                } else {
                    "lists tests"
                };
                found.push(make(
                    format!(
                        "profile.{profile}.scripts {key} = \"{name}\" command = {command}{filter}{platform}"
                    ),
                    format!("{stage} through the wrapper {name}, command {command}"),
                ));
            }
        }
    }
    for (profile, section) in checked(parsed) {
        if let Some(filter) = section.get("default-filter") {
            found.push(make(
                format!("profile.{profile}.default-filter = {filter}"),
                format!(
                    "limits a plain nextest run to the tests matching {filter}; the nextest step Velnor generates ignores it"
                ),
            ));
        }
        if let Some(retries) = section.get("retries").filter(|r| retries_tests(r)) {
            found.push(make(
                format!(
                    "profile.{profile} retries = {retries}{}",
                    flaky_result(section)
                ),
                "retries every test in the profile, so a flaky failure passes".to_owned(),
            ));
        }
        if let Some(timeout) = section.get("slow-timeout").filter(|t| passes_on_timeout(t)) {
            found.push(make(
                format!("profile.{profile} slow-timeout = {timeout}"),
                "counts a test that times out as passed".to_owned(),
            ));
        }
        if let Some(args) = section.get("run-extra-args") {
            found.push(make(
                format!("profile.{profile} run-extra-args = {args}"),
                "passes extra arguments to every test binary in the profile".to_owned(),
            ));
        }
        found.extend(overrides(profile, section, make));
    }
    found
}

fn overrides(profile: &str, section: &Value, make: &Make<'_>) -> Vec<Weakening> {
    let mut found = Vec::new();
    for entry in entries(section, "overrides") {
        let selector = entry
            .get("filter")
            .map_or_else(|| "every test".to_owned(), ToString::to_string);
        let platform = entry
            .get("platform")
            .map_or_else(String::new, |platform| format!(" platform = {platform}"));
        let head = format!("profile.{profile}.overrides filter = {selector}{platform}");
        if let Some(retries) = entry.get("retries").filter(|r| retries_tests(r)) {
            found.push(make(
                format!("{head} retries = {retries}{}", flaky_result(entry)),
                format!("retries the tests matching {selector}, so a flaky failure there passes"),
            ));
        }
        if let Some(timeout) = entry.get("slow-timeout").filter(|t| passes_on_timeout(t)) {
            found.push(make(
                format!("{head} slow-timeout = {timeout}"),
                format!("counts a test matching {selector} that times out as passed"),
            ));
        }
        if let Some(args) = entry.get("run-extra-args") {
            found.push(make(
                format!("{head} run-extra-args = {args}"),
                format!("passes extra arguments to the test binaries matching {selector}"),
            ));
        }
    }
    found
}
