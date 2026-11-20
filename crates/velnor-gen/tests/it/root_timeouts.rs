// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;
use velnor_rust::detect;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const ROOTS: [&str; 2] = ["Cargo.toml", "lab/Cargo.toml"];
const HELPER: &str = "0.1.109";

/// A repository limit of 30 minutes, 90 for the `lab` root, and a parallel lane of `lab` that
/// sets its own 120 minutes.
const LANE: &str = "timeout_minutes = 30

[root_timeouts]
lab = 90

[[test_lane]]
root = 'lab'
features = ['gpu']
job = 'parallel'
timeout_minutes = 120
";

fn jobs_of(config: &str, helper: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let text = workflow(&view_with(&ROOTS, config, helper)?)?;
    Ok(document(&text)?["jobs"].clone())
}

fn refusal(config: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let error = workflow(&view_with(&ROOTS, config, helper)?)
        .err()
        .ok_or("the configuration was accepted")?;
    Ok(error.to_string())
}

fn timeouts_with_prefix(jobs: &serde_json::Value, prefix: &str) -> Vec<(String, i64)> {
    jobs.as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| id.starts_with(prefix))
        .map(|(id, job)| {
            (
                id.clone(),
                job["timeout-minutes"].as_i64().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn the_root_and_lane_keys_parse_into_the_configuration() -> TestResult {
    let facts = detect(&view_with(&ROOTS, LANE, HELPER)?)?;
    let config = &facts.config;
    assert_eq!(config.timeout_minutes, 30);
    assert_eq!(
        config.root_timeouts.get(&Some(RepoPath::new("lab")?)),
        Some(&90)
    );
    assert_eq!(config.root_timeouts.len(), 1);
    assert_eq!(config.test_lanes[0].timeout_minutes, Some(120));
    Ok(())
}

#[test]
fn a_root_key_names_the_repository_root_with_dot_and_takes_a_trailing_slash() -> TestResult {
    let config = "[root_timeouts]\n'.' = 45\n'lab/' = 90\n";
    let facts = detect(&view_with(&ROOTS, config, HELPER)?)?;
    assert_eq!(facts.config.root_timeouts.get(&None), Some(&45));
    assert_eq!(
        facts.config.root_timeouts.get(&Some(RepoPath::new("lab")?)),
        Some(&90)
    );
    Ok(())
}

#[test]
fn an_out_of_range_duplicate_or_unknown_root_is_refused_with_its_name() -> TestResult {
    for (config, mention) in [
        ("[root_timeouts]\nlab = 0\n", "root_timeouts"),
        ("[root_timeouts]\nlab = 361\n", "root_timeouts"),
        ("[root_timeouts]\nlab = 90\n'lab/' = 91\n", "lab"),
        ("[root_timeouts]\nmissing = 90\n", "missing"),
        (
            "[[test_lane]]\nroot = '.'\nfeatures = ['gpu']\ntimeout_minutes = 0\n",
            "timeout_minutes",
        ),
        (
            "[[test_lane]]\nroot = '.'\nfeatures = ['gpu']\ntimeout_minutes = 361\n",
            "timeout_minutes",
        ),
    ] {
        let text = refusal(config, HELPER)?;
        assert!(text.contains(mention), "{config}: {text}");
    }
    Ok(())
}

#[test]
fn a_helper_older_than_the_timeout_keys_is_refused() -> TestResult {
    for config in [LANE, "[root_timeouts]\nlab = 90\n"] {
        let text = refusal(config, "0.1.108")?;
        assert!(text.contains("0.1.109"), "{text}");
        jobs_of(config, HELPER)?;
    }
    Ok(())
}

#[test]
fn a_parallel_lane_job_takes_its_own_limit_and_the_root_its_own() -> TestResult {
    let jobs = jobs_of(LANE, HELPER)?;
    let lane = timeouts_with_prefix(&jobs, "rust-lane-gpu");
    assert_eq!(lane.len(), 1, "{lane:?}");
    assert_eq!(lane[0].1, 120, "{lane:?}");
    let lab = timeouts_with_prefix(&jobs, "rust-lab");
    assert!(!lab.is_empty(), "{jobs}");
    assert!(lab.iter().all(|(_, minutes)| *minutes == 90), "{lab:?}");
    let root = timeouts_with_prefix(&jobs, "rust-root");
    assert!(root.iter().all(|(_, minutes)| *minutes == 30), "{root:?}");
    Ok(())
}

const GROUPS: &str = "timeout_minutes = 30

[root_timeouts]
lab = 90

[[test_groups]]
name = 'database'
root = 'lab'
filter = 'package(package-1) and test(/^db_/)'
needs = ['docker']

[[test_groups]]
name = 'slow'
root = 'lab'
filter = 'package(package-1) and test(~slow)'
timeout_minutes = 45
";

#[test]
fn a_groups_archive_build_takes_the_root_limit_and_a_group_its_own() -> TestResult {
    let jobs = jobs_of(GROUPS, HELPER)?;
    let expected = [
        ("rust-test-lab", 90),
        ("rust-lint-lab", 90),
        ("rust-test-database-lab", 90),
        ("rust-test-default-lab", 90),
        ("rust-test-slow-lab", 45),
        ("rust-root", 30),
    ];
    for (id, minutes) in expected {
        let job = jobs.get(id).ok_or_else(|| format!("no job {id}: {jobs}"))?;
        assert_eq!(job["timeout-minutes"], minutes, "{id}");
    }
    Ok(())
}

#[test]
fn a_parallel_lane_without_its_own_limit_takes_the_root_limit() -> TestResult {
    let config = "timeout_minutes = 30\n[root_timeouts]\nlab = 90\n\n[[test_lane]]\nroot = 'lab'\nfeatures = ['gpu']\njob = 'parallel'\n";
    let jobs = jobs_of(config, HELPER)?;
    assert_eq!(jobs["rust-lane-gpu-lab"]["timeout-minutes"], 90, "{jobs}");
    Ok(())
}

#[test]
fn an_inline_lane_raises_only_the_root_job_that_runs_it() -> TestResult {
    let config = "timeout_minutes = 30\n[root_timeouts]\nlab = 90\n\n[[test_lane]]\nroot = 'lab'\nfeatures = ['gpu']\ntimeout_minutes = 120\n";
    let jobs = jobs_of(config, HELPER)?;
    assert_eq!(jobs["rust-lab"]["timeout-minutes"], 120, "{jobs}");
    assert_eq!(jobs["rust-root"]["timeout-minutes"], 30, "{jobs}");
    let lower = "timeout_minutes = 30\n[root_timeouts]\nlab = 90\n\n[[test_lane]]\nroot = 'lab'\nfeatures = ['gpu']\ntimeout_minutes = 60\n";
    assert_eq!(jobs_of(lower, HELPER)?["rust-lab"]["timeout-minutes"], 90);
    Ok(())
}
