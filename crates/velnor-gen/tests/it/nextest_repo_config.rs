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

const TEN_SECOND_TESTS: [&str; 8] = [
    "audit::build_directory::a_build_command_naming_a_build_directory_is_refused",
    "audit::groups::a_real_grouped_build_passes_the_audit",
    "audit::groups::a_real_grouped_build_passes_the_audit_through_a_symlinked_root",
    "audit::per_root::a_per_root_audit_compares_only_the_lock_lines_of_its_own_packages",
    "plan_diff::a_check_id_outside_the_identifier_alphabet_is_refused",
    "plan_diff::every_recorded_case_matches_the_legacy_op",
    "plan_main::a_push_reads_the_base_lock_while_it_fits_and_selects_everything_past_the_byte_budget",
    "io_plan_groups::plan_outputs_one_run_or_skip_per_group_and_root",
];

fn covers(filter: &str, name: &str) -> bool {
    if filter.contains("test(") {
        names_test(filter, name)
    } else {
        filter.contains("package(=velnor-ci)")
    }
}

/// The period that nextest applies to `name`: the first override that sets a slow-timeout and matches it.
fn first_timeout_period(entries: &[toml::Table], name: &str) -> Option<String> {
    entries
        .iter()
        .filter(|entry| entry.contains_key("slow-timeout"))
        .find(|entry| covers(filter(entry), name))
        .and_then(|entry| {
            entry
                .get("slow-timeout")?
                .get("period")?
                .as_str()
                .map(str::to_owned)
        })
}

#[test]
fn the_scoped_ten_second_tier_is_the_timeout_nextest_applies_to_each_named_slow_test() -> TestResult {
    for profile in PROFILES {
        let entries = overrides(profile)?;
        for name in TEN_SECOND_TESTS {
            assert_eq!(
                first_timeout_period(&entries, name).as_deref(),
                Some("10s"),
                "profile {profile}: {name} does not get the scoped 10 s tier"
            );
        }
    }
    Ok(())
}
