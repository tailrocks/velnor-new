// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::TargetKind;
use velnor_rust::{CONFIG_PATH, detect};

use crate::facts::{TestResult, package, repo};
use crate::pipeline::{HELPER_LOCK, workflow};
use crate::test_groups::{job, needs, run_of, step, steps, view_with};
use crate::yaml_tree::document;

pub(crate) const HELPER: &str = "0.1.110";

pub(crate) fn group(extra: &str) -> String {
    format!(
        "[[test_groups]]\nname = 'database'\nfilter = 'package(package-0) and test(/^db_/)'\n{extra}"
    )
}

pub(crate) fn rendered_with(
    config: &str,
    helper: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(document(&workflow(&view_with(
        &["Cargo.toml"],
        config,
        helper,
    )?)?)?)
}

pub(crate) fn refused(config: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let error = workflow(&view_with(&["Cargo.toml"], config, helper)?)
        .err()
        .ok_or("the configuration was accepted")?;
    Ok(error.to_string())
}

fn partition_ids(workflow: &Value) -> Vec<String> {
    let mut ids: Vec<String> = workflow["jobs"]
        .as_object()
        .into_iter()
        .flat_map(|jobs| jobs.keys().cloned())
        .filter(|id| id.starts_with("rust-test-database"))
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn partitions_are_parsed_and_bounded() -> TestResult {
    let facts = detect(&view_with(
        &["Cargo.toml"],
        &group("partitions = 4\n"),
        HELPER,
    )?)?;
    assert_eq!(facts.config.test_groups[0].partitions, Some(4));
    let facts = detect(&view_with(&["Cargo.toml"], &group(""), HELPER)?)?;
    assert_eq!(facts.config.test_groups[0].partitions, None);
    for bad in ["0", "17", "100"] {
        let message = refused(&group(&format!("partitions = {bad}\n")), HELPER)?;
        assert!(
            message.contains("test_groups.partitions"),
            "{bad}: {message}"
        );
    }
    for accepted in ["1", "2", "16"] {
        rendered_with(&group(&format!("partitions = {accepted}\n")), HELPER)?;
    }
    refused(&group("partitions = 'four'\n"), HELPER)?;
    Ok(())
}

#[test]
fn a_partitioned_group_renders_one_job_per_partition_with_its_hash_slice() -> TestResult {
    let workflow = rendered_with(&group("partitions = 3\ntest_threads = 2\n"), HELPER)?;
    assert_eq!(
        partition_ids(&workflow),
        [
            "rust-test-database-1",
            "rust-test-database-2",
            "rust-test-database-3"
        ]
    );
    for index in 1..=3 {
        let job = job(&workflow, &format!("rust-test-database-{index}"))?;
        let run = run_of(job, "Test")?;
        assert!(
            run.contains(&format!("--partition hash:{index}/3")),
            "{run}"
        );
        assert!(run.contains("--test-threads 2"), "{run}");
        assert!(
            run.contains("-E \"(($TEST_FILTER) and (package(package-0) and test(/^db_/)))\""),
            "{run}"
        );
        assert!(needs(job).contains(&"rust-plan") && needs(job).contains(&"rust-test"));
    }
    Ok(())
}

#[test]
fn a_group_without_partitions_or_with_one_stays_a_single_job() -> TestResult {
    for extra in ["", "partitions = 1\n"] {
        let workflow = rendered_with(&group(extra), HELPER)?;
        assert_eq!(partition_ids(&workflow), ["rust-test-database"], "{extra}");
        let run = run_of(job(&workflow, "rust-test-database")?, "Test")?;
        assert!(!run.contains("--partition"), "{run}");
    }
    Ok(())
}

#[test]
fn the_required_job_needs_every_partition_and_never_the_bare_group() -> TestResult {
    let workflow = rendered_with(
        &format!(
            "{}\n{}",
            group("partitions = 2\n"),
            "[[test_groups]]\nname = 'shell'\nfilter = 'all()'\n"
        ),
        HELPER,
    )?;
    let required = needs(job(&workflow, "required")?);
    for id in [
        "rust-test-database-1",
        "rust-test-database-2",
        "rust-test-shell",
        "rust-test-default",
    ] {
        assert!(required.contains(&id), "{id} not in {required:?}");
    }
    assert!(!required.contains(&"rust-test-database"), "{required:?}");
    Ok(())
}

#[test]
fn every_partition_carries_the_group_gate_and_reports_test_counts_like_the_group_did() -> TestResult
{
    let selecting = "paths = ['app/**']\nevents = ['push']\n";
    let single = rendered_with(&group(selecting), HELPER)?;
    let split = rendered_with(&group(&format!("{selecting}partitions = 2\n")), HELPER)?;
    let one = job(&single, "rust-test-database")?;
    for index in 1..=2 {
        let part = job(&split, &format!("rust-test-database-{index}"))?;
        assert_eq!(part["if"], one["if"], "partition {index}");
        assert_eq!(part["outputs"], one["outputs"], "partition {index}");
        assert!(part["outputs"].as_object().is_some_and(|o| !o.is_empty()));
        assert_eq!(part["permissions"], one["permissions"]);
        assert_eq!(part["timeout-minutes"], one["timeout-minutes"]);
    }
    Ok(())
}

#[test]
fn a_group_whose_partition_id_collides_with_another_group_is_refused() -> TestResult {
    let config = format!(
        "{}\n[[test_groups]]\nname = 'database-1'\nfilter = 'all()'\n",
        group("partitions = 2\n")
    );
    let message = refused(&config, HELPER)?;
    assert!(
        message.contains("test_groups.name") && message.contains("database-1"),
        "{message}"
    );
    Ok(())
}

#[test]
fn partitions_on_a_root_without_nextest_tests_are_refused_naming_the_key() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[[test]]\nname = \"custom\"\nharness = false\n";
    let mise =
        "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.81", HELPER, 1);
    let config = "[[test_groups]]\nname = 'db'\nfilter = 'all()'\npartitions = 2\n";
    let error = workflow(&repo(
        vec![("Cargo.toml", vec![a])],
        &[
            ("mise.toml", mise),
            ("a/Cargo.toml", manifest),
            (CONFIG_PATH.as_str(), config),
            (".velnor/rust-ci.lock", &lock),
        ],
    )?)
    .err()
    .ok_or("a partitioned group without nextest tests was accepted")?
    .to_string();
    assert!(
        error.contains("partitions") && error.contains("nextest") && error.contains("db"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_helper_older_than_the_partitions_release_refuses_the_key() -> TestResult {
    for helper in ["0.1.90", "0.1.107"] {
        let message = refused(&group("partitions = 2\n"), helper)?;
        assert!(
            message.contains(helper)
                && message.contains("0.1.110")
                && message.contains("partitions"),
            "{message}"
        );
    }
    refused(&group(""), "0.1.107").err().ok_or("")?;
    Ok(())
}

#[test]
fn partition_job_steps_keep_the_group_download_and_setup() -> TestResult {
    let workflow = rendered_with(&group("partitions = 2\nneeds = ['docker']\n"), HELPER)?;
    for index in 1..=2 {
        let job = job(&workflow, &format!("rust-test-database-{index}"))?;
        assert!(step(job, "Check that Docker answers").is_ok());
        assert!(steps(job).iter().any(|s| s["name"] == "Download the tests"));
    }
    Ok(())
}

#[test]
fn an_empty_partition_fails_on_every_partition_instead_of_passing_silently() -> TestResult {
    let config = "[[test_groups]]\nname = 'database'\nfilter = 'package(package-0) and test(=no_such_test_anywhere)'\npartitions = 2\n";
    let workflow = rendered_with(config, HELPER)?;
    for index in 1..=2 {
        let run = run_of(job(&workflow, &format!("rust-test-database-{index}"))?, "Test")?;
        assert!(run.contains("--no-tests=fail"), "partition {index}: {run}");
        assert!(!run.contains("--no-tests=pass"), "partition {index}: {run}");
    }
    Ok(())
}
