// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::facts::TestResult;

const PROFILES: [&str; 2] = ["default", "ci"];
const SERIAL_GROUP: &str = "velnor-ci-serial";
const PLAN_MAIN_GROUPS_ATOM: &str = "test(plan_main_groups::)";
const VELNOR_CI: &str = "package(=velnor-ci)";

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
        let selection = filter(&grouped[0]);
        assert!(
            selection.contains(PLAN_MAIN_GROUPS_ATOM),
            "profile {profile}: {PLAN_MAIN_GROUPS_ATOM} is missing from the {SERIAL_GROUP} filter"
        );
        assert!(
            !selection.contains("not ") && !selection.contains('!'),
            "profile {profile}: the {SERIAL_GROUP} filter negates"
        );
    }
    Ok(())
}

const SCOPED_NAMED_TESTS: [&str; 13] = [
    "configuration_targets::configuration_targets_rejects_incomplete_workspace_metadata_before_checks",
    "configuration_targets::configuration_targets_reports_unknown_when_feature_contexts_are_ambiguous",
    "configuration_targets::contexts::configuration_targets_scopes_hazardous_edges_to_transitive_dependencies",
    "configuration_targets::contexts::configuration_targets_uses_global_uncertainty_for_unbounded_graphs",
    "run::run_keeps_each_stream_and_every_byte_and_appends_one_record_per_command",
    "io_target_case::checkout_under_a_cargo_ancestor_still_saves_its_default_target",
    "audit::build_directory::a_build_command_naming_a_build_directory_is_refused",
    "audit::groups::a_real_grouped_build_passes_the_audit",
    "audit::groups::a_real_grouped_build_passes_the_audit_through_a_symlinked_root",
    "audit::per_root::a_per_root_audit_compares_only_the_lock_lines_of_its_own_packages",
    "plan_diff::a_check_id_outside_the_identifier_alphabet_is_refused",
    "plan_main::a_push_reads_the_base_lock_while_it_fits_and_selects_everything_past_the_byte_budget",
    "io_plan_groups::plan_outputs_one_run_or_skip_per_group_and_root",
];

fn exact(name: &str) -> String {
    format!("test(={name})")
}

fn slow_timeouts(profile: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut chain = overrides(profile)?;
    if profile != "default" {
        chain.extend(overrides("default")?);
    }
    Ok(chain
        .iter()
        .filter_map(|entry| {
            let period = entry.get("slow-timeout")?.get("period")?.as_str()?;
            Some((filter(entry).to_owned(), period.to_owned()))
        })
        .collect())
}

fn assert_tier(tier: &str, atoms: &[String]) -> TestResult {
    for profile in PROFILES {
        let timeouts = slow_timeouts(profile)?;
        for atom in atoms {
            let periods: Vec<&str> = timeouts
                .iter()
                .filter(|(selection, _)| selection.contains(atom.as_str()))
                .map(|(_, period)| period.as_str())
                .collect();
            assert!(
                !periods.is_empty() && periods.iter().all(|period| *period == tier),
                "profile {profile}: {atom} carries {periods:?}, not only {tier}"
            );
        }
    }
    Ok(())
}

fn exact_atoms(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| exact(name)).collect()
}

#[test]
fn the_named_slow_velnor_ci_tests_keep_the_thirty_second_tier_main_gives_them() -> TestResult {
    let mut atoms = exact_atoms(&SCOPED_NAMED_TESTS);
    atoms.push(PLAN_MAIN_GROUPS_ATOM.to_owned());
    assert_tier("30s", &atoms)
}

#[test]
fn a_velnor_ci_test_without_a_scoped_tier_keeps_the_two_second_default() -> TestResult {
    for profile in PROFILES {
        let velnor_ci: Vec<(String, String)> = slow_timeouts(profile)?
            .into_iter()
            .filter(|(selection, _)| selection.contains(VELNOR_CI))
            .collect();
        let package_wide: Vec<usize> = velnor_ci
            .iter()
            .enumerate()
            .filter(|(_, (selection, _))| !selection.contains("test("))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(
            package_wide,
            [velnor_ci.len() - 1],
            "profile {profile}: the package-wide velnor-ci tier must be one entry, after every scoped tier"
        );
        assert_eq!(velnor_ci[velnor_ci.len() - 1].1, "2s", "profile {profile}");
    }
    Ok(())
}

#[test]
fn the_measured_slow_velnor_ci_families_get_the_thirty_second_tier() -> TestResult {
    assert_tier(
        "30s",
        &[
            "test(/^delta_freshness::/)".to_owned(),
            "test(/^plan_git::/)".to_owned(),
            "test(/^io_report::/)".to_owned(),
            exact(
                "mise_version_plan::a_change_to_mise_version_runs_the_velnor_gen_tests_that_read_it",
            ),
        ],
    )
}

#[test]
fn the_replays_of_every_recorded_case_get_the_sixty_second_tier() -> TestResult {
    assert_tier(
        "60s",
        &exact_atoms(&[
            "plan_diff::every_recorded_case_matches_the_legacy_op",
            "resolve_qualification::every_recorded_case_matches_the_legacy_op",
            "io_plan_docs::plan_treats_sources_over_the_total_size_bound_as_possibly_holding_docs",
        ]),
    )
}
