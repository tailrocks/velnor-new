// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use velnor_gen::{GenerateError, expected_files};
use velnor_model::{EventKind, JobId, RepoPath, RepoView, VerdictSpec};
use velnor_rust::{Need, decide, detect, lint_runs};

use crate::facts::TestResult;
use crate::lint_lane::{LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};

const DECLARED: &str = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'lint-generator-samples'\ntools = ['jactionlint']\ninputs = ['hk.pkl', 'mise.toml', 'tools/**']\n\n[[lint]]\nkind = 'jactionlint'\nconfig = '.github/jactionlint.yaml'\n";
const OUTPUT: &str = "needs.rust-plan.outputs.lint_hk == 'run'";

fn repo(roots: &[&str], config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    crate::facts::lock_pinned_tools(&mut repo)?;
    Ok(repo)
}

fn jobs(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"].clone())
}

fn multi() -> Result<String, Box<dyn std::error::Error>> {
    workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], DECLARED)?)
}

fn single() -> Result<String, Box<dyn std::error::Error>> {
    workflow(&repo(&["Cargo.toml"], DECLARED)?)
}

fn spec(text: &str) -> Result<VerdictSpec, String> {
    let line = text
        .split("GATE_SPEC: '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .ok_or("no gate spec")?;
    let words: Vec<&str> = line.split(' ').collect();
    let after = |flag: &str| -> Vec<String> {
        words
            .windows(2)
            .filter(|pair| pair[0] == flag)
            .map(|pair| pair[1].to_owned())
            .collect()
    };
    let plan = after("--plan").pop().ok_or("no plan")?;
    VerdictSpec::parse(&plan, &after("--job"), &after("--tests"))
}

fn judged(
    text: &str,
    hk_output: &str,
    hk_result: &str,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let spec = spec(text)?;
    let plan_outputs = jobs(text)?["rust-plan"]["outputs"].clone();
    let mut needs = BTreeMap::new();
    for job in &spec.jobs {
        let result = match job.id.as_str() {
            "lint-hk" => hk_result,
            id if id.starts_with("lint-") => "success",
            _ if job.when.is_empty() => "success",
            _ => "skipped",
        };
        needs.insert(
            job.id.clone(),
            Need {
                result: Some(result.to_owned()),
                outputs: BTreeMap::new(),
            },
        );
    }
    let outputs = plan_outputs
        .as_object()
        .ok_or("no plan outputs")?
        .keys()
        .map(|key| {
            let value = match key.as_str() {
                "lint_hk" => hk_output,
                key if key.ends_with("_mode") => "none",
                _ => "skip",
            };
            (key.clone(), value.to_owned())
        })
        .collect();
    needs.insert(
        JobId::new("rust-plan")?,
        Need {
            result: Some("success".to_owned()),
            outputs,
        },
    );
    let verdict = decide(&spec, &needs, Some("pull_request"));
    Ok((verdict.passed, verdict.table()))
}

#[test]
fn a_lint_with_inputs_waits_for_the_plan_and_runs_only_when_the_plan_says_run() -> TestResult {
    let all = jobs(&multi()?)?;
    let job = &all["lint-hk"];
    assert_eq!(job["needs"], serde_json::json!(["rust-plan"]), "{job}");
    let condition = job["if"].as_str().ok_or("no job condition")?;
    assert!(condition.contains(OUTPUT), "{condition}");
    let plan = all["rust-plan"]["outputs"]
        .as_object()
        .ok_or("no plan outputs")?;
    assert!(plan.contains_key("lint_hk"), "{plan:?}");
    assert!(!plan.contains_key("lint_jactionlint"), "{plan:?}");
    Ok(())
}

#[test]
fn a_lint_without_inputs_renders_exactly_as_before() -> TestResult {
    let all = jobs(&multi()?)?;
    let job = &all["lint-jactionlint"];
    assert!(job["needs"].is_null(), "{job}");
    assert!(
        !job["if"].as_str().unwrap_or_default().contains("lint_"),
        "{job}"
    );
    let plain = workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], LINT)?)?;
    assert!(!plain.contains("lint_"), "no lint output without inputs");
    assert!(
        !plain.contains("=lint:"),
        "no lint condition without inputs"
    );
    Ok(())
}

#[test]
fn the_required_gate_treats_a_lint_the_plan_skipped_as_passing() -> TestResult {
    let text = multi()?;
    let spec = spec(&text)?;
    let hk = spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == "lint-hk")
        .ok_or("the gate spec lacks lint-hk")?;
    assert_eq!(hk.when.len(), 1, "{hk:?}");
    let other = spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == "lint-jactionlint")
        .ok_or("the gate spec lacks lint-jactionlint")?;
    assert!(other.when.is_empty(), "{other:?}");

    let (green, table) = judged(&text, "skip", "skipped")?;
    assert!(green, "{table}");
    let (green, table) = judged(&text, "run", "success")?;
    assert!(green, "{table}");
    for (output, result) in [
        ("run", "skipped"),
        ("run", "failure"),
        ("skip", "failure"),
        ("skip", "cancelled"),
    ] {
        let (green, table) = judged(&text, output, result)?;
        assert!(!green, "{output} {result}: {table}");
    }
    Ok(())
}

#[test]
fn a_single_job_workflow_plans_inside_the_lint_job_and_gates_its_steps() -> TestResult {
    let text = single()?;
    let all = jobs(&text)?;
    let job = &all["lint-hk"];
    assert!(job["needs"].is_null(), "{job}");
    assert!(job["if"].as_str().is_some_and(|c| !c.contains("lint_hk")));
    let steps = job["steps"].as_array().ok_or("no steps")?;
    let plan = steps
        .iter()
        .position(|step| step["id"] == "plan")
        .ok_or("the lint job runs no plan step")?;
    let checkout = steps
        .iter()
        .position(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/checkout"))
        })
        .ok_or("no checkout")?;
    assert!(checkout < plan);
    assert_eq!(steps[checkout]["with"]["fetch-depth"], 2);
    let gated = "steps.plan.outputs.lint_hk == 'run'";
    assert!(steps.len() > plan + 1);
    for step in &steps[plan + 1..] {
        let when = step["if"].as_str().unwrap_or_default();
        assert!(when.contains(gated), "{step}");
    }
    let other = &all["lint-jactionlint"];
    assert!(other["steps"].as_array().is_some_and(|steps| {
        steps.iter().all(|step| {
            step["id"] != "plan" && !step["if"].as_str().unwrap_or_default().contains("lint_")
        })
    }));
    assert!(spec(&text)?.jobs.iter().all(|job| job.when.is_empty()));
    Ok(())
}

#[test]
fn a_helper_older_than_the_key_is_refused_with_the_gate_error() -> TestResult {
    let mut repo = repo(&["Cargo.toml"], DECLARED)?;
    pin_helper(&mut repo, "0.1.109")?;
    let error = expected_files(&repo).err();
    assert!(
        matches!(
            error,
            Some(GenerateError::ConfigKeyHelperTooOld {
                key: "lint.inputs",
                first: (0, 1, 110),
                ..
            })
        ),
        "{error:?}"
    );
    pin_helper(&mut repo, "0.1.110")?;
    assert!(expected_files(&repo).is_ok());
    Ok(())
}

#[test]
fn lint_inputs_that_name_nothing_or_collide_are_rejected() -> TestResult {
    let cases = [
        (
            "[[lint]]\nkind = 'jactionlint'\ninputs = []\n",
            "lint.inputs",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\ninputs = ['a/**/b']\n",
            "lint.inputs",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\ninputs = ['a*b']\n",
            "lint.inputs",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\nid = 'a-b'\ninputs = ['x']\n\n[[lint]]\nkind = 'jactionlint'\nid = 'a_b'\ninputs = ['y']\n",
            "a_b",
        ),
    ];
    for (config, needle) in cases {
        let text = format!(
            "{}",
            detect(&repo(&["Cargo.toml"], config)?)
                .err()
                .ok_or(config)?
        );
        assert!(text.contains(needle), "{config}: {text}");
    }
    Ok(())
}

fn changed(paths: &[&str]) -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
    paths
        .iter()
        .map(|path| RepoPath::new(*path).map_err(Into::into))
        .collect()
}

fn runs_of(
    event: EventKind,
    paths: Option<&[&str]>,
) -> Result<Vec<(String, bool)>, Box<dyn std::error::Error>> {
    let facts = detect(&repo(&["Cargo.toml"], DECLARED)?)?;
    let paths = paths.map(changed).transpose()?;
    Ok(lint_runs(&facts, event, paths.as_ref())
        .into_iter()
        .map(|(slug, run)| (slug.to_string(), run))
        .collect())
}

fn hk(event: EventKind, paths: Option<&[&str]>) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(runs_of(event, paths)?
        .into_iter()
        .find(|(slug, _)| slug == "hk")
        .ok_or("no plan decision for hk")?
        .1)
}

#[test]
fn a_pull_request_that_changes_no_input_skips_the_lint() -> TestResult {
    assert!(!hk(
        EventKind::PullRequest,
        Some(&["src/lib.rs", "README.md"])
    )?);
    assert!(!hk(EventKind::PullRequest, Some(&[]))?);
    assert!(!hk(
        EventKind::PullRequest,
        Some(&["a/hk.pkl", "toolsmith/x"])
    )?);
    Ok(())
}

#[test]
fn a_pull_request_that_changes_an_input_runs_the_lint() -> TestResult {
    for path in ["hk.pkl", "mise.toml", "tools/a/b.sh"] {
        assert!(
            hk(EventKind::PullRequest, Some(&["README.md", path]))?,
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn the_workflow_and_the_config_that_generate_the_lint_always_run_it() -> TestResult {
    for path in [".github/workflows/rust-ci.yml", ".velnor/rust-ci.toml"] {
        assert!(hk(EventKind::PullRequest, Some(&[path]))?, "{path}");
    }
    Ok(())
}

#[test]
fn every_event_but_a_pull_request_with_a_known_change_runs_the_lint() -> TestResult {
    for event in [
        EventKind::Push,
        EventKind::MergeGroup,
        EventKind::Schedule,
        EventKind::WorkflowDispatch,
    ] {
        assert!(hk(event, Some(&["README.md"]))?, "{}", event.name());
        assert!(hk(event, None)?, "{}", event.name());
    }
    assert!(
        hk(EventKind::PullRequest, None)?,
        "unknown change fails open"
    );
    Ok(())
}

#[test]
fn a_lint_without_inputs_has_no_plan_decision() -> TestResult {
    let all = runs_of(EventKind::PullRequest, Some(&["README.md"]))?;
    assert_eq!(all, [("hk".to_owned(), false)]);
    Ok(())
}

#[test]
fn the_declared_config_and_probe_of_a_lint_count_as_its_inputs() -> TestResult {
    let config = "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\nprobe = 'scripts/probe.sh'\ninputs = ['rules/**']\n";
    let facts = detect(&repo(&["Cargo.toml"], config)?)?;
    let at = |path: &str| -> Result<bool, Box<dyn std::error::Error>> {
        let paths = changed(&[path])?;
        let runs = lint_runs(&facts, EventKind::PullRequest, Some(&paths));
        Ok(*runs.values().next().ok_or("no decision")?)
    };
    assert!(at(".alint.yml")?);
    assert!(at("scripts/probe.sh")?);
    assert!(at("rules/a.yml")?);
    assert!(!at("src/lib.rs")?);
    Ok(())
}
