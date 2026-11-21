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

fn names_test(filter: &str, name: &str) -> bool {
    filter.split("test(").skip(1).any(|rest| {
        let pattern = rest.split(')').next().unwrap_or("");
        if let Some(exact) = pattern.strip_prefix('=') {
            exact == name
        } else if let Some(prefix) = pattern
            .strip_prefix("/^")
            .and_then(|rest| rest.strip_suffix('/'))
        {
            name.starts_with(prefix)
        } else {
            name.contains(pattern)
        }
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

const SCOPED_NAMED_TESTS: [&str; 19] = [
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
    "plan_main_groups::a_merge_group_runs_a_group_that_only_names_events",
    "plan_main_groups::a_merge_group_skips_a_group_whose_paths_the_diff_does_not_touch",
    "plan_main_groups::a_merge_group_runs_a_group_whose_paths_the_diff_touches",
    "plan_main_groups::a_merge_group_whose_diff_cannot_be_computed_runs_every_group",
    "plan_main_groups::a_push_to_main_runs_every_group_whatever_its_paths_or_events",
    "plan_main_groups::on_a_push_to_main_the_groups_follow_the_selection_of_the_default_lanes",
];

fn covers(filter: &str, name: &str) -> bool {
    filter.contains("package(=velnor-ci)")
        && (!filter.contains("test(") || names_test(filter, name))
}

fn effective_timeout_period(
    profile: &str,
    name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut chain = overrides(profile)?;
    if profile != "default" {
        chain.extend(overrides("default")?);
    }
    Ok(chain
        .iter()
        .filter(|entry| entry.contains_key("slow-timeout"))
        .find(|entry| covers(filter(entry), name))
        .and_then(|entry| {
            entry
                .get("slow-timeout")?
                .get("period")?
                .as_str()
                .map(str::to_owned)
        }))
}

fn assert_tier(tier: &str, names: &[&str]) -> TestResult {
    for profile in PROFILES {
        for name in names {
            assert_eq!(
                effective_timeout_period(profile, name)?.as_deref(),
                Some(tier),
                "profile {profile}: {name} does not get the {tier} tier"
            );
        }
    }
    Ok(())
}

#[test]
fn the_named_slow_velnor_ci_tests_keep_the_thirty_second_tier_main_gives_them() -> TestResult {
    assert_tier("30s", &SCOPED_NAMED_TESTS)
}

#[test]
fn a_velnor_ci_test_without_a_scoped_tier_keeps_the_two_second_default() -> TestResult {
    assert_tier(
        "2s",
        &[
            "gate::current_run::a_changed_listing_refuses_with_a_retry_diagnosis",
            "tools_digest::a_changed_component_list_still_changes_the_digest",
        ],
    )
}

#[test]
fn the_measured_slow_velnor_ci_families_get_the_thirty_second_tier() -> TestResult {
    assert_tier(
        "30s",
        &[
            "delta_freshness::changed_member_rechecks_every_cached_package",
            "plan_git::a_modified_file_selects_its_package_with_no_token",
            "io_report::provenance::links::tracked_link_cycle_cannot_prove_source_contents",
            "mise_version_plan::a_change_to_mise_version_runs_the_velnor_gen_tests_that_read_it",
        ],
    )
}

#[test]
fn the_replays_of_every_recorded_case_get_the_sixty_second_tier() -> TestResult {
    assert_tier(
        "60s",
        &[
            "plan_diff::every_recorded_case_matches_the_legacy_op",
            "resolve_qualification::every_recorded_case_matches_the_legacy_op",
            "io_plan_docs::plan_treats_sources_over_the_total_size_bound_as_possibly_holding_docs",
        ],
    )
}
