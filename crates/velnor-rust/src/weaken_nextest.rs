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

const SCALARS: &[&str] = &[
    "default-filter",
    "retries",
    "slow-timeout",
    "run-extra-args",
];
const DEFAULT_PROFILE: &str = "default";

struct Reach<'a> {
    name: &'a str,
    section: &'a Value,
    checked: bool,
    keys: Vec<&'static str>,
    overrides: Vec<(usize, &'static str)>,
}

impl Reach<'_> {
    fn sets(&self, key: &str) -> Option<&Value> {
        self.section
            .get(key)
            .filter(|_| self.checked || self.keys.contains(&key))
    }

    fn live(&self, index: usize, key: &str) -> bool {
        self.checked || self.overrides.contains(&(index, key))
    }
}

fn section<'a>(parsed: &'a Table, name: &str) -> Option<(&'a str, &'a Value)> {
    profiles(parsed)
        .find(|(profile, _)| profile.as_str() == name)
        .map(|(profile, value)| (profile.as_str(), value))
}

fn chain<'a>(parsed: &'a Table, name: &'a str) -> Vec<&'a str> {
    let mut seen = vec![name];
    let mut at = name;
    loop {
        let next = section(parsed, at)
            .and_then(|(_, value)| value.get("inherits"))
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_PROFILE);
        if seen.contains(&next) {
            return seen;
        }
        seen.push(next);
        at = next;
    }
}

fn reach<'a, 'b>(
    parsed: &'a Table,
    reached: &'b mut Vec<Reach<'a>>,
    name: &str,
) -> Option<&'b mut Reach<'a>> {
    if let Some(at) = reached.iter().position(|seen| seen.name == name) {
        return reached.get_mut(at);
    }
    let (name, section) = section(parsed, name)?;
    reached.push(Reach {
        name,
        section,
        checked: false,
        keys: Vec::new(),
        overrides: Vec::new(),
    });
    reached.last_mut()
}

fn reached(parsed: &Table) -> Vec<Reach<'_>> {
    let mut reached: Vec<Reach<'_>> = profiles(parsed)
        .filter(|(profile, _)| CHECKED_PROFILES.contains(&profile.as_str()))
        .map(|(profile, section)| Reach {
            name: profile.as_str(),
            section,
            checked: true,
            keys: Vec::new(),
            overrides: Vec::new(),
        })
        .collect();
    let checked: Vec<&str> = reached.iter().map(|seen| seen.name).collect();
    for name in checked {
        let chain = chain(parsed, name);
        for key in SCALARS {
            let setter = chain.iter().copied().find(|name| {
                section(parsed, name).is_some_and(|(_, value)| value.get(*key).is_some())
            });
            if let Some(profile) = setter.and_then(|name| reach(parsed, &mut reached, name)) {
                profile.keys.push(key);
            }
        }
        let mut covered: Vec<&str> = Vec::new();
        for name in &chain {
            let Some(profile) = reach(parsed, &mut reached, name) else {
                continue;
            };
            for (index, entry) in entries(profile.section, "overrides").enumerate() {
                for key in SCALARS.iter().filter(|key| entry.get(**key).is_some()) {
                    if !covered.contains(key) {
                        profile.overrides.push((index, key));
                    }
                }
                if entry.get("filter").is_none() && entry.get("platform").is_none() {
                    covered.extend(SCALARS.iter().filter(|key| entry.get(**key).is_some()));
                }
            }
        }
    }
    reached
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
    let reached = reached(parsed);
    for Reach {
        name: profile,
        section,
        ..
    } in &reached
    {
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
    for reach in &reached {
        let (profile, section) = (reach.name, reach.section);
        let sets = |key: &str| reach.sets(key);
        if let Some(filter) = sets("default-filter") {
            found.push(make(
                format!("profile.{profile}.default-filter = {filter}"),
                format!(
                    "limits the default nextest run to the tests matching {filter}; the generated test step applies it, so only a test group or lane that selects a test runs it"
                ),
            ));
        }
        if let Some(retries) = sets("retries").filter(|r| retries_tests(r)) {
            found.push(make(
                format!(
                    "profile.{profile} retries = {retries}{}",
                    flaky_result(section)
                ),
                "retries every test in the profile, so a flaky failure passes".to_owned(),
            ));
        }
        if let Some(timeout) = sets("slow-timeout").filter(|t| passes_on_timeout(t)) {
            found.push(make(
                format!("profile.{profile} slow-timeout = {timeout}"),
                "counts a test that times out as passed".to_owned(),
            ));
        }
        if let Some(args) = sets("run-extra-args") {
            found.push(make(
                format!("profile.{profile} run-extra-args = {args}"),
                "passes extra arguments to every test binary in the profile".to_owned(),
            ));
        }
        found.extend(overrides(reach, make));
    }
    found
}

fn overrides(reach: &Reach<'_>, make: &Make<'_>) -> Vec<Weakening> {
    let profile = reach.name;
    let mut found = Vec::new();
    for (index, entry) in entries(reach.section, "overrides").enumerate() {
        let live = |key: &str| reach.live(index, key).then(|| entry.get(key)).flatten();
        if let (Some(filter), Some(platform)) = (live("default-filter"), entry.get("platform")) {
            found.push(make(
                format!("profile.{profile}.overrides default-filter = {filter} platform = {platform}"),
                format!(
                    "limits the default nextest run to the tests matching {filter} on the matching platform; the generated test step applies it, so only a test group or lane that selects a test runs it"
                ),
            ));
        }
        let selector = entry
            .get("filter")
            .map_or_else(|| "every test".to_owned(), ToString::to_string);
        let platform = entry
            .get("platform")
            .map_or_else(String::new, |platform| format!(" platform = {platform}"));
        let head = format!("profile.{profile}.overrides filter = {selector}{platform}");
        if let Some(retries) = live("retries").filter(|r| retries_tests(r)) {
            found.push(make(
                format!("{head} retries = {retries}{}", flaky_result(entry)),
                format!("retries the tests matching {selector}, so a flaky failure there passes"),
            ));
        }
        if let Some(timeout) = live("slow-timeout").filter(|t| passes_on_timeout(t)) {
            found.push(make(
                format!("{head} slow-timeout = {timeout}"),
                format!("counts a test matching {selector} that times out as passed"),
            ));
        }
        if let Some(args) = live("run-extra-args") {
            found.push(make(
                format!("{head} run-extra-args = {args}"),
                format!("passes extra arguments to the test binaries matching {selector}"),
            ));
        }
    }
    found
}
