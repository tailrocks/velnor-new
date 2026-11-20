// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CheckConfigurations, InputRule, RepoPath};

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::select::{assert_selects, glob, name, path, two_roots, unknown};
use crate::test_groups::{DATABASE, view_with};
use crate::yaml_tree::document;

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const HELPER: &str = "0.1.110";
const STEP: &str = "velnor-ci check-configurations";
const KEY: &str =
    "[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/configurations.json'\n";

fn tracked(
    config: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut view = view_with(ROOTS, config, helper)?;
    for file in [
        "ci/configurations.json",
        "-ci.json",
        "docs/configurations.json",
    ] {
        view.tracked_paths.insert(RepoPath::new(file)?);
    }
    Ok(view)
}

fn jobs_of(config: &str, helper: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let text = workflow(&tracked(config, helper)?)?;
    Ok(document(&text)?["jobs"].clone())
}

fn refusal(config: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let error = workflow(&tracked(config, helper)?)
        .err()
        .ok_or("the configuration was accepted")?;
    Ok(error.to_string())
}

fn found(jobs: &serde_json::Value) -> Vec<(String, usize, Option<usize>, serde_json::Value)> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = job["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let tests = steps.iter().position(|step| step["id"] == "test");
        for (index, step) in steps.iter().enumerate() {
            if step["name"] == STEP {
                found.push((id.clone(), index, tests, step.clone()));
            }
        }
    }
    found
}

#[test]
fn without_the_key_no_job_calls_check_configurations() -> TestResult {
    assert_eq!(found(&jobs_of("", HELPER)?).len(), 0);
    Ok(())
}

#[test]
fn the_key_renders_one_step_in_the_test_job_of_its_root_before_the_tests() -> TestResult {
    let steps = found(&jobs_of(KEY, HELPER)?);
    assert_eq!(steps.len(), 1, "{steps:?}");
    let (id, index, tests, step) = &steps[0];
    assert_eq!(id, "rust-root");
    assert!(tests.is_some_and(|tests| index < &tests), "{steps:?}");
    assert_eq!(
        step["run"],
        ". \"$VELNOR_EXEC\" check-configurations --configurations=ci/configurations.json"
    );
    assert!(step["working-directory"].is_null(), "{step}");
    Ok(())
}

#[test]
fn the_configurations_file_is_optional_and_a_nested_root_runs_in_its_folder() -> TestResult {
    let steps = found(&jobs_of(
        "[[check_configurations]]\nroot = 'tools'\n",
        HELPER,
    )?);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].0, "rust-tools");
    assert_eq!(steps[0].3["run"], ". \"$VELNOR_EXEC\" check-configurations");
    assert_eq!(steps[0].3["working-directory"], "tools");
    Ok(())
}

#[test]
fn a_helper_older_than_the_subcommand_is_refused_with_the_key_and_release() -> TestResult {
    let text = refusal(KEY, "0.1.109")?;
    assert!(text.contains("check_configurations"), "{text}");
    assert!(text.contains("0.1.110"), "{text}");
    jobs_of(KEY, HELPER)?;
    Ok(())
}

#[test]
fn an_unknown_root_or_an_unusable_file_path_is_refused_with_its_name() -> TestResult {
    for (config, mention) in [
        ("[[check_configurations]]\nroot = 'missing'\n", "missing"),
        ("[[check_configurations]]\n", "root"),
        (
            "[[check_configurations]]\nroot = '.'\nconfigurations = '../x.json'\n",
            "check_configurations.configurations",
        ),
        (
            "[[check_configurations]]\nroot = '.'\nconfigurations = 'a b.json'\n",
            "check_configurations.configurations",
        ),
    ] {
        let text = refusal(config, HELPER)?;
        assert!(text.contains(mention), "{config}: {text}");
    }
    Ok(())
}

#[test]
fn a_path_that_starts_with_a_hyphen_reaches_the_subcommand_as_the_value() -> TestResult {
    let steps = found(&jobs_of(
        "[[check_configurations]]\nroot = '.'\nconfigurations = '-ci.json'\n",
        HELPER,
    )?);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(
        steps[0].3["run"],
        ". \"$VELNOR_EXEC\" check-configurations --configurations=-ci.json"
    );
    Ok(())
}

#[test]
fn an_untracked_file_or_one_a_non_inputs_rule_ignores_is_refused() -> TestResult {
    for (config, mention) in [
        (
            "[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/missing.json'\n",
            "ci/missing.json",
        ),
        (
            "non_inputs = ['ci/**']\n[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/configurations.json'\n",
            "non_inputs",
        ),
    ] {
        let text = refusal(config, HELPER)?;
        assert!(text.contains(mention), "{config}: {text}");
    }
    Ok(())
}

#[test]
fn with_archive_test_groups_the_check_runs_once_in_the_build_job_not_in_the_runners() -> TestResult
{
    let groups = DATABASE.replace("[[test_groups]]", "[[test_groups]]\nroot = '.'");
    let config = format!("{groups}\n{KEY}");
    let all = jobs_of(&config, HELPER)?;
    let jobs = found(&all);
    let ids: Vec<&str> = jobs.iter().map(|job| job.0.as_str()).collect();
    assert_eq!(ids, ["rust-test-root"], "{jobs:?}");
    let archive = all["rust-test-root"]["steps"]
        .as_array()
        .and_then(|steps| {
            steps.iter().position(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("archive"))
            })
        })
        .ok_or("no archive step in the build job")?;
    assert!(jobs[0].1 < archive, "{jobs:?} archive at {archive}");
    Ok(())
}

#[test]
fn a_change_to_the_configurations_file_selects_work_even_under_a_default_ignore() {
    let mut facts = two_roots();
    facts.config.check_configurations = vec![CheckConfigurations {
        root: None,
        configurations: Some(path("docs/configurations.json")),
    }];
    let file = "docs/configurations.json";
    assert_selects(
        &facts,
        vec![(&[file][..], vec![unknown(file), unknown(file)])],
    );
}

#[test]
fn sparse_checkout_never_drops_the_configurations_file_of_the_root_that_runs_the_check()
-> TestResult {
    let config = "sparse_checkout = true\n[[check_configurations]]\nroot = 'tools'\nconfigurations = 'ci.json'\n";
    let mut view = tracked(config, HELPER)?;
    view.tracked_paths.insert(RepoPath::new("tools/ci.json")?);
    let jobs = document(&workflow(&view)?)?["jobs"].clone();
    let steps = jobs["rust-tools"]["steps"].as_array().ok_or("no steps")?;
    let listing = steps
        .iter()
        .find_map(|step| step["with"]["sparse-checkout"].as_str())
        .ok_or("the tools job has no sparse checkout")?;
    assert!(listing.contains("/tools/"), "{listing}");
    assert_eq!(found(&jobs).len(), 1);
    let root = jobs["rust-root"]["steps"].as_array().ok_or("no steps")?;
    assert!(
        root.iter()
            .all(|step| step["with"]["sparse-checkout"].is_null()),
        "the top-level root keeps a full checkout"
    );
    Ok(())
}

#[test]
fn an_inputs_rule_for_another_root_does_not_hide_a_change_to_the_configurations_file() {
    let mut facts = two_roots();
    facts.config.check_configurations = vec![CheckConfigurations {
        root: None,
        configurations: Some(path("docs/configurations.json")),
    }];
    facts.config.inputs = vec![InputRule {
        glob: glob("docs/*"),
        packages: vec![name("cli")],
    }];
    let file = "docs/configurations.json";
    assert_selects(
        &facts,
        vec![(&[file][..], vec![unknown(file), unknown(file)])],
    );
}

#[test]
fn a_symlinked_configurations_file_or_folder_is_refused() -> TestResult {
    let config =
        "[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/configurations.json'\n";
    for link in ["ci/configurations.json", "ci"] {
        let mut view = tracked(config, HELPER)?;
        view.symlinks.insert(RepoPath::new(link)?);
        let error = workflow(&view).err().ok_or("the symlink was accepted")?;
        assert!(
            error.to_string().contains("symbolic link"),
            "{link}: {error}"
        );
    }
    Ok(())
}

#[test]
fn a_parallel_test_lane_does_not_repeat_the_check_in_its_own_job() -> TestResult {
    let config =
        format!("{KEY}[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\njob = 'parallel'\n");
    let steps = found(&jobs_of(&config, HELPER)?);
    let ids: Vec<&str> = steps.iter().map(|step| step.0.as_str()).collect();
    assert_eq!(ids, ["rust-root"], "{steps:?}");
    Ok(())
}

#[test]
fn a_root_without_a_test_command_still_gets_the_check_in_a_compiling_job() -> TestResult {
    let mut view = tracked(KEY, HELPER)?;
    for metadata in view.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            for target in &mut member.targets {
                target.tests = false;
            }
        }
    }
    let steps = found(&document(&workflow(&view)?)?["jobs"]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    Ok(())
}

#[test]
fn a_grouped_root_runs_the_check_in_the_group_target_directory() -> TestResult {
    let config = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n[[check_configurations]]\nroot = 'tools/a'\n";
    let view = view_with(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        config,
        HELPER,
    )?;
    let steps = found(&document(&workflow(&view)?)?["jobs"]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(
        steps[0].3["env"]["CARGO_TARGET_DIR"], "${{ github.workspace }}/target-velnor/tools",
        "{steps:?}"
    );
    Ok(())
}

#[test]
fn a_configurations_path_that_is_a_submodule_is_refused_like_a_symlink() -> TestResult {
    let mut view = tracked(KEY, HELPER)?;
    view.gitlinks
        .insert(RepoPath::new("ci/configurations.json")?);
    let error = workflow(&view)
        .err()
        .ok_or("the submodule path was accepted")?;
    assert!(error.to_string().contains("submodule"), "{error}");
    Ok(())
}

#[test]
fn a_repeated_check_entry_is_refused_so_the_check_runs_once() -> TestResult {
    let text = refusal(&format!("{KEY}{KEY}"), HELPER)?;
    assert!(text.contains("check_configurations"), "{text}");
    Ok(())
}
