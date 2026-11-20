// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::facts::TestResult;

const PROFILES: [&str; 2] = ["default", "ci"];
const SERIAL_GROUP: &str = "velnor-ci-serial";

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn overrides(profile: &str) -> Result<Vec<toml::Table>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(root().join(".config/nextest.toml"))?;
    let config: toml::Table = toml::from_str(&text)?;
    let list = config
        .get("profile")
        .and_then(|profiles| profiles.get(profile))
        .and_then(|profile| profile.get("overrides"))
        .and_then(toml::Value::as_array)
        .ok_or("the profile has no overrides")?;
    Ok(list
        .iter()
        .filter_map(|entry| entry.as_table().cloned())
        .collect())
}

fn filter(entry: &toml::Table) -> &str {
    entry
        .get("filter")
        .and_then(toml::Value::as_str)
        .unwrap_or("")
}

#[test]
fn no_override_gives_every_velnor_ci_test_a_longer_timeout() -> TestResult {
    for profile in PROFILES {
        for entry in overrides(profile)? {
            let bare = matches!(
                filter(&entry).replace(['"', '\''], "").trim(),
                "package(=velnor-ci)"
            );
            assert!(
                !(bare && entry.contains_key("slow-timeout")),
                "profile {profile}: a package-wide slow-timeout hides the 2 s default of velnor-ci: {entry:?}"
            );
        }
    }
    Ok(())
}

fn names_test(filter: &str, name: &str) -> bool {
    filter.split("test(").skip(1).any(|rest| {
        let pattern = rest.split(')').next().unwrap_or("");
        pattern
            .strip_prefix('=')
            .map_or_else(|| name.contains(pattern), |exact| exact == name)
    })
}

fn tests_of(file: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let path = root().join("crates/velnor-ci/tests/it").join(file);
    let text = fs::read_to_string(path)?;
    let module = file.trim_end_matches(".rs");
    let mut names = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "#[test]" {
            let declared = lines.next().unwrap_or("");
            if let Some(name) = declared
                .strip_prefix("fn ")
                .and_then(|rest| rest.split('(').next())
            {
                names.push(format!("{module}::{name}"));
            }
        }
    }
    Ok(names)
}

#[test]
fn the_velnor_ci_serial_group_holds_every_plan_main_groups_test() -> TestResult {
    let names = tests_of("plan_main_groups.rs")?;
    assert!(names.len() >= 6, "{names:?}");
    for profile in PROFILES {
        let grouped: Vec<toml::Table> = overrides(profile)?
            .into_iter()
            .filter(|entry| {
                entry.get("test-group").and_then(toml::Value::as_str) == Some(SERIAL_GROUP)
            })
            .collect();
        assert_eq!(grouped.len(), 1, "profile {profile}");
        for name in &names {
            assert!(
                names_test(filter(&grouped[0]), name),
                "profile {profile}: {name} reaches the mise wrapper but is outside {SERIAL_GROUP}"
            );
        }
    }
    Ok(())
}
