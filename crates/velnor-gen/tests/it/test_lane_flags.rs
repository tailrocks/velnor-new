// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::Value;

use crate::facts::TestResult;
use crate::feature_lanes::{lane_view, steps_named};
use crate::pipeline::workflow;

const FLAGS_HELPER: &str = "0.1.108";
const ALL: &str = "[[test_lane]]\nroot = '.'\nall_features = true\npackages = ['package-0']\nfilter = 'test(~all::)'\nlint = true\ndoc = true\n";
const NO_DEFAULT: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['a', 'b']\nno_default_features = true\npackages = ['package-0']\nfilter = 'test(~nodefault::)'\nlint = true\ndoc = true\n";
const PARALLEL: &str = "job = 'parallel'\n";

fn jobs_at(helper: &str, config: &str) -> Result<Value, Box<dyn Error>> {
    let repo = lane_view(helper, config)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn run_of(jobs: &Value, name: &str) -> Result<String, Box<dyn Error>> {
    let found = steps_named(jobs, name);
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    Ok(found[0].2["run"].as_str().ok_or("no run")?.to_owned())
}

fn refused(helper: &str, config: &str) -> Result<String, Box<dyn Error>> {
    match lane_view(helper, config) {
        Ok(repo) => match workflow(&repo) {
            Ok(text) => Err(format!("the workflow was generated:\n{text}").into()),
            Err(error) => Ok(error.to_string()),
        },
        Err(error) => Err(error),
    }
}

#[test]
fn an_all_features_lane_passes_the_flag_to_its_test_lint_doc_and_guard_steps() -> TestResult {
    let jobs = jobs_at(FLAGS_HELPER, ALL)?;
    let test = run_of(&jobs, "Test (all features)")?;
    assert!(
        test.contains("nextest run --workspace --locked") && test.contains(" --all-features"),
        "{test}"
    );
    assert!(!test.contains("--features"), "{test}");
    assert!(run_of(&jobs, "Lint (all features)")?.contains("--all-features"));
    assert!(run_of(&jobs, "Documentation (all features)")?.contains("--all-features"));
    let guard = run_of(&jobs, "Test lane guard (all features)")?;
    assert!(
        guard.contains("velnor-ci lane-guard --all-features"),
        "{guard}"
    );
    assert!(!guard.contains("--features"), "{guard}");
    Ok(())
}

#[test]
fn a_no_default_features_lane_combines_the_flag_with_its_features_in_every_step() -> TestResult {
    let jobs = jobs_at(FLAGS_HELPER, NO_DEFAULT)?;
    let name = "no default features, features: a,b";
    let test = run_of(&jobs, &format!("Test ({name})"))?;
    assert!(
        test.contains(" --no-default-features --features 'a,b'"),
        "{test}"
    );
    assert!(
        run_of(&jobs, &format!("Lint ({name})"))?
            .contains(" --no-default-features --features 'a,b'")
    );
    assert!(
        run_of(&jobs, &format!("Documentation ({name})"))?
            .contains(" --no-default-features --features 'a,b'")
    );
    let guard = run_of(&jobs, &format!("Test lane guard ({name})"))?;
    assert!(
        guard.contains("velnor-ci lane-guard --no-default-features --features 'a,b' --filter"),
        "{guard}"
    );
    Ok(())
}

#[test]
fn a_no_default_features_lane_may_name_no_features() -> TestResult {
    let config = NO_DEFAULT.replace("features = ['a', 'b']\n", "");
    let jobs = jobs_at(FLAGS_HELPER, &config)?;
    let test = run_of(&jobs, "Test (no default features)")?;
    assert!(test.contains(" --no-default-features"), "{test}");
    assert!(!test.contains("--features"), "{test}");
    Ok(())
}

#[test]
fn a_plain_feature_lane_keeps_its_rendered_steps_and_names() -> TestResult {
    let config = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\nall_features = false\npackages = ['package-0']\nfilter = 'test(~local::)'\n";
    let jobs = jobs_at(FLAGS_HELPER, config)?;
    let test = run_of(&jobs, "Test (features: local-mode)")?;
    assert!(test.contains(" --features 'local-mode'"), "{test}");
    assert!(!test.contains("--all-features"), "{test}");
    Ok(())
}

#[test]
fn all_features_conflicts_with_a_features_list_and_with_no_default_features() -> TestResult {
    let with_features = ALL.replace(
        "all_features = true\n",
        "all_features = true\nfeatures = ['a']\n",
    );
    let error = refused(FLAGS_HELPER, &with_features)?;
    assert!(
        error.contains("all_features") && error.contains("features"),
        "{error}"
    );
    let with_no_default = ALL.replace(
        "all_features = true\n",
        "all_features = true\nno_default_features = true\n",
    );
    let error = refused(FLAGS_HELPER, &with_no_default)?;
    assert!(
        error.contains("all_features") && error.contains("no_default_features"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_lane_with_no_features_and_no_flag_is_still_refused() -> TestResult {
    let config = "[[test_lane]]\nroot = '.'\npackages = ['package-0']\nfilter = 'test(~a)'\n";
    let error = refused(FLAGS_HELPER, config)?;
    assert!(error.contains("test_lane.features"), "{error}");
    Ok(())
}

#[test]
fn the_flags_need_the_helper_that_reads_them() -> TestResult {
    for config in [ALL, NO_DEFAULT] {
        for helper in ["0.1.93", "0.1.106"] {
            let error = workflow(&lane_view(helper, config)?)
                .err()
                .ok_or("an old helper was accepted")?;
            assert!(
                error.to_string().contains("pin-helper 0.1.107"),
                "{helper}: {error}"
            );
        }
    }
    let written_false = ALL.replace(
        "all_features = true",
        "all_features = false\nfeatures = ['a']",
    );
    assert!(
        workflow(&lane_view("0.1.106", &written_false)?).is_err(),
        "a written flag needs the new helper even when false"
    );
    Ok(())
}

#[test]
fn lanes_that_differ_only_by_a_flag_get_distinct_jobs_and_both_lint_steps() -> TestResult {
    let plain = "[[test_lane]]\nroot = '.'\nfeatures = ['a']\npackages = ['package-0']\nfilter = 'test(~plain::)'\nlint = true\n";
    let all = "[[test_lane]]\nroot = '.'\nall_features = true\npackages = ['package-0']\nfilter = 'test(~all::)'\nlint = true\n";
    let no_default = "[[test_lane]]\nroot = '.'\nfeatures = ['a']\nno_default_features = true\npackages = ['package-0']\nfilter = 'test(~nd::)'\nlint = true\n";
    let config = format!("{plain}{PARALLEL}{all}{PARALLEL}{no_default}{PARALLEL}");
    let text = workflow(&lane_view(FLAGS_HELPER, &config)?)?;
    let jobs = crate::yaml_tree::document(&text)?["jobs"].clone();
    let lanes: Vec<&String> = jobs
        .as_object()
        .into_iter()
        .flat_map(|map| map.keys())
        .filter(|id| id.starts_with("rust-lane"))
        .collect();
    assert_eq!(lanes.len(), 3, "{lanes:?}");
    let mut sorted = lanes.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "the lanes share a job id: {lanes:?}");
    for id in &lanes {
        assert!(text.contains(&format!("{id}.")), "no cache key names {id}");
    }
    for name in [
        "Lint (features: a)",
        "Lint (all features)",
        "Lint (no default features, features: a)",
    ] {
        assert_eq!(steps_named(&jobs, name).len(), 1, "{name}");
    }
    Ok(())
}
