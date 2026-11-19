// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const LANE_STEP: &str = "Test (features: local-mode)";

pub(crate) fn lane_view(
    helper: &str,
    config: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn jobs_of(config: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let repo = lane_view("0.1.86", config)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn steps_named(jobs: &serde_json::Value, name: &str) -> Vec<(String, usize, serde_json::Value)> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        for (index, step) in job["steps"].as_array().into_iter().flatten().enumerate() {
            if step["name"] == name {
                found.push((id.clone(), index, step.clone()));
            }
        }
    }
    found
}

#[test]
fn a_feature_lane_runs_its_own_nextest_step_with_the_features_and_the_filter() -> TestResult {
    let jobs = jobs_of(LANE)?;
    let lane = steps_named(&jobs, LANE_STEP);
    assert_eq!(lane.len(), 1, "{lane:?}");
    let (id, position, step) = &lane[0];
    assert_eq!(id, "rust-root");
    let run = step["run"].as_str().ok_or("no run")?;
    assert!(
        run.contains("nextest run --workspace --locked")
            && run.contains(" --features 'local-mode' "),
        "{run}"
    );
    assert!(
        run.contains("-E \"($TEST_FILTER) and (test(~local::) or binary(local_host_public))\""),
        "{run}"
    );
    assert!(step["id"].is_null(), "{step}");
    let defaults: Vec<_> = steps_named(&jobs, "Test")
        .into_iter()
        .filter(|(job, ..)| job == "rust-root")
        .collect();
    assert_eq!(defaults.len(), 1, "{defaults:?}");
    let default_run = defaults[0].2["run"].as_str().ok_or("no run")?;
    assert!(!default_run.contains("--features"), "{default_run}");
    assert!(defaults[0].1 < *position, "the default tests run first");
    Ok(())
}

#[test]
fn a_feature_lane_without_a_filter_runs_the_whole_selected_set_with_the_features() -> TestResult {
    let config = LANE.replace(
        "filter = 'test(~local::) or binary(local_host_public)'\n",
        "",
    );
    let lane = steps_named(&jobs_of(&config)?, LANE_STEP);
    let run = lane[0].2["run"].as_str().ok_or("no run")?;
    assert!(run.contains("--features 'local-mode'"), "{run}");
    assert!(run.contains("-E \"$TEST_FILTER\""), "{run}");
    Ok(())
}

#[test]
fn lanes_of_other_roots_and_several_features_render_per_root() -> TestResult {
    let config = "[[test_lane]]\nroot = 'tools'\nfeatures = ['a', 'b']\n";
    let jobs = jobs_of(config)?;
    let lane = steps_named(&jobs, "Test (features: a,b)");
    assert_eq!(lane.len(), 1, "{lane:?}");
    assert_eq!(lane[0].0, "rust-tools");
    assert_eq!(lane[0].2["working-directory"], "tools");
    assert!(
        lane[0].2["run"]
            .as_str()
            .ok_or("no run")?
            .contains("--features 'a,b'")
    );
    Ok(())
}

#[test]
fn in_the_groups_shape_the_feature_lane_runs_where_rust_is_installed() -> TestResult {
    let config = format!("shape = 'groups'\n{LANE}");
    let jobs = jobs_of(&config)?;
    let lane = steps_named(&jobs, LANE_STEP);
    assert_eq!(lane.len(), 1, "{lane:?}");
    assert_eq!(lane[0].0, "rust-test-root");
    Ok(())
}

#[test]
fn a_feature_lane_that_is_not_one_safe_command_is_rejected() -> TestResult {
    for (config, mention) in [
        (LANE.replace("root = '.'\n", ""), "root"),
        (LANE.replace("root = '.'", "root = 'missing'"), "missing"),
        (LANE.replace("['local-mode']", "[]"), "test_lane.features"),
        (LANE.replace("['local-mode']", "['']"), "test_lane.features"),
        (
            LANE.replace("['local-mode']", "['a b']"),
            "test_lane.features",
        ),
        (
            LANE.replace("['local-mode']", "['a,b']"),
            "test_lane.features",
        ),
        (
            LANE.replace("['local-mode']", "['a', 'a']"),
            "test_lane.features",
        ),
        (
            LANE.replace("'test(~local::) or binary(local_host_public)'", "''"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) |"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) ;"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) & binary(x) or"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~\"x\")"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~$X)"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~`x`)"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) or if"),
            "test_lane.filter",
        ),
        (
            LANE.replace(
                "filter = 'test(~local::) or binary(local_host_public)'",
                "filter = '''\ntest(~a)\nor test(~b)\n'''",
            ),
            "test_lane.filter",
        ),
        (format!("{LANE}extra = 1\n"), "extra"),
        (format!("{LANE}lint = 'yes'\n"), "expected a boolean"),
        (format!("{LANE}doc = 1\n"), "expected a boolean"),
    ] {
        let repo = lane_view("0.1.86", &config)?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("the configuration was accepted: {config}"))?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}

const FULL_LANE: &str =
    "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\nlint = true\ndoc = true\n";

fn run_of(
    jobs: &serde_json::Value,
    name: &str,
    job: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let found: Vec<_> = steps_named(jobs, name)
        .into_iter()
        .filter(|(id, ..)| id == job)
        .collect();
    assert_eq!(found.len(), 1, "{name} in {job}: {found:?}");
    Ok(found[0].2["run"].as_str().ok_or("no run")?.to_owned())
}

#[test]
fn a_lane_with_lint_and_doc_runs_clippy_and_rustdoc_with_the_features_in_the_same_job() -> TestResult
{
    let jobs = jobs_of(FULL_LANE)?;
    let clippy = run_of(&jobs, "Lint (features: local-mode)", "rust-root")?;
    let default_clippy = run_of(&jobs, "Lint", "rust-root")?;
    assert_eq!(
        clippy,
        default_clippy.replace(" -- -D warnings", " --features 'local-mode' -- -D warnings"),
        "the lane clippy is the default clippy plus the features"
    );
    assert!(!default_clippy.contains("--features"), "{default_clippy}");
    let doc = run_of(&jobs, "Documentation (features: local-mode)", "rust-root")?;
    let default_doc = run_of(&jobs, "Documentation", "rust-root")?;
    assert_eq!(doc, format!("{default_doc} --features 'local-mode'"));
    assert!(!default_doc.contains("--features"), "{default_doc}");
    let lane = steps_named(&jobs, LANE_STEP);
    assert_eq!(lane.len(), 1, "{lane:?}");
    for name in [
        "Lint (features: local-mode)",
        "Documentation (features: local-mode)",
    ] {
        let step = &steps_named(&jobs, name)[0];
        assert_eq!(step.2["working-directory"], lane[0].2["working-directory"]);
    }
    Ok(())
}

#[test]
fn lint_and_doc_are_independent_and_default_to_off() -> TestResult {
    let only_lint = jobs_of(&FULL_LANE.replace("doc = true\n", ""))?;
    assert_eq!(
        steps_named(&only_lint, "Lint (features: local-mode)").len(),
        1
    );
    assert_eq!(
        steps_named(&only_lint, "Documentation (features: local-mode)").len(),
        0
    );
    let only_doc = jobs_of(&FULL_LANE.replace("lint = true\n", ""))?;
    assert_eq!(
        steps_named(&only_doc, "Lint (features: local-mode)").len(),
        0
    );
    assert_eq!(
        steps_named(&only_doc, "Documentation (features: local-mode)").len(),
        1
    );
    let neither = jobs_of(LANE)?;
    assert_eq!(
        steps_named(&neither, "Lint (features: local-mode)").len(),
        0
    );
    assert_eq!(
        steps_named(&neither, "Documentation (features: local-mode)").len(),
        0
    );
    let off = jobs_of(&format!("{LANE}lint = false\ndoc = false\n"))?;
    assert_eq!(off, neither);
    Ok(())
}

#[test]
fn the_verdict_job_needs_the_job_that_runs_the_lane_checks() -> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let jobs = jobs_of(&format!("{shape}{FULL_LANE}"))?;
        let needs: Vec<_> = jobs["required"]["needs"]
            .as_array()
            .ok_or("no needs")?
            .iter()
            .filter_map(|need| need.as_str())
            .collect();
        for name in [
            "Lint (features: local-mode)",
            "Documentation (features: local-mode)",
        ] {
            let found = steps_named(&jobs, name);
            assert_eq!(found.len(), 1, "{shape}{name}: {found:?}");
            assert!(
                needs.contains(&found[0].0.as_str()),
                "{shape}{name}: {needs:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn in_every_shape_the_lane_checks_run_in_the_job_that_has_rust() -> TestResult {
    for (shape, job) in [
        ("one-job", "rust-root"),
        ("lint-test", "rust-test-root"),
        ("groups", "rust-test-root"),
    ] {
        let jobs = jobs_of(&format!("shape = '{shape}'\n{FULL_LANE}"))?;
        for name in [
            LANE_STEP,
            "Lint (features: local-mode)",
            "Documentation (features: local-mode)",
        ] {
            let found = steps_named(&jobs, name);
            assert_eq!(found.len(), 1, "{shape} {name}: {found:?}");
            assert_eq!(found[0].0, job, "{shape} {name}");
        }
    }
    Ok(())
}

fn lane_filter_error(filter: &str) -> Result<String, Box<dyn std::error::Error>> {
    let config = LANE.replace(
        "test(~local::) or binary(local_host_public)",
        &filter.replace('\'', "''"),
    );
    let repo = lane_view("0.1.86", &config)?;
    let error = workflow(&repo)
        .err()
        .ok_or_else(|| format!("the filter was accepted: {filter:?}"))?;
    Ok(error.to_string())
}

#[test]
fn every_character_that_could_leave_the_quoted_filter_is_refused_on_its_own() -> TestResult {
    for forbidden in ['"', '$', '`', '\\', '|', '&', ';', '<'] {
        let error = lane_filter_error(&format!("test(~a{forbidden}b)"))?;
        assert!(error.contains("test_lane.filter"), "{forbidden}: {error}");
    }
    Ok(())
}

#[test]
fn a_filter_ending_in_a_backslash_is_refused() -> TestResult {
    for filter in ["test(~a)\\", "test(~a) \\", "test(~a\\)"] {
        let error = lane_filter_error(filter)?;
        assert!(error.contains("test_lane.filter"), "{filter}: {error}");
    }
    Ok(())
}

#[test]
fn every_shell_keyword_is_refused_as_a_word_of_the_filter() -> TestResult {
    for word in ["if", "for", "while", "case", "coproc"] {
        let error = lane_filter_error(&format!("test(~a) or {word}"))?;
        assert!(error.contains("test_lane.filter"), "{word}: {error}");
    }
    Ok(())
}

#[test]
fn a_filter_with_unbalanced_parentheses_is_refused_and_a_balanced_one_is_not() -> TestResult {
    for filter in [
        "test(~a)) or (test(~b)",
        "test(~a) or (test(~b)",
        "test(~a))",
        "(test(~a)",
        ")(",
    ] {
        let error = lane_filter_error(filter)?;
        assert!(error.contains("test_lane.filter"), "{filter}: {error}");
    }
    let jobs = jobs_of(&LANE.replace(
        "test(~local::) or binary(local_host_public)",
        "(test(~a) or (binary(b))) and not test(~c)",
    ))?;
    assert_eq!(steps_named(&jobs, LANE_STEP).len(), 1);
    Ok(())
}

#[test]
fn a_feature_lane_fails_when_its_filter_selects_no_test_and_the_default_step_does_not() -> TestResult
{
    let jobs = jobs_of(LANE)?;
    let lane = steps_named(&jobs, LANE_STEP);
    let run = lane[0].2["run"].as_str().ok_or("no run")?;
    assert!(run.ends_with("--no-tests=fail"), "{run}");
    assert!(!run.contains("--no-tests=pass"), "{run}");
    let defaults = steps_named(&jobs, "Test");
    let default_run = defaults[0].2["run"].as_str().ok_or("no run")?;
    assert!(default_run.ends_with("--no-tests=pass"), "{default_run}");
    Ok(())
}

#[test]
fn a_feature_lane_runs_with_profile_ci_exactly_when_the_root_defines_it() -> TestResult {
    let lane_run = |nextest: Option<&str>| -> Result<String, Box<dyn std::error::Error>> {
        let mut repo = lane_view("0.1.86", LANE)?;
        if let Some(text) = nextest {
            let path = RepoPath::new(".config/nextest.toml")?;
            repo.tracked_paths.insert(path.clone());
            repo.files.insert(path, text.as_bytes().to_vec());
        }
        let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
        let lane = steps_named(&jobs, LANE_STEP);
        Ok(lane[0].2["run"].as_str().ok_or("no run")?.to_owned())
    };
    let with_ci = lane_run(Some("[profile.ci]\nslow-timeout = \"60s\"\n"))?;
    assert!(
        with_ci.contains("--features 'local-mode' --profile ci --no-fail-fast"),
        "{with_ci}"
    );
    let without = lane_run(None)?;
    assert!(!without.contains("--profile"), "{without}");
    Ok(())
}
