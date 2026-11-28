// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::lint_lane::{LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};

const KNOB: &str = "[ci]\nreuse_pr_verdict = true\n\n";
const REUSED: &str = "contains(format(' {0} ', needs.rust-plan.outputs.reused), ' ";

fn repo(roots: &[&str], knob: bool, helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let config = format!("{}{LINT}", if knob { KNOB } else { "" });
    let mut repo = view(roots, Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn locked(roots: &[&str], helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut locked = repo(roots, true, helper)?;
    crate::facts::lock_pinned_tools(&mut locked)?;
    Ok(locked)
}

fn jobs(roots: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let text = workflow(&repo(roots, true, "0.1.117")?)?;
    Ok(crate::yaml_tree::document(&text)?["jobs"].clone())
}

fn step<'a>(job: &'a Value, name: &str) -> Result<&'a Value, String> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["name"] == name)
        .ok_or_else(|| format!("no step {name}"))
}

fn condition(job: &Value, name: &str) -> Result<String, String> {
    Ok(step(job, name)?["if"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[test]
fn the_knob_off_leaves_the_workflow_byte_identical_and_the_gate_without_reuse() -> TestResult {
    let roots = ["Cargo.toml", "lab/Cargo.toml"];
    let plain = workflow(&repo(&roots, false, "0.1.117")?)?;
    assert!(!plain.contains("verdict"), "{plain}");
    assert!(!plain.contains("--reuse"), "{plain}");
    let on = workflow(&repo(&roots, true, "0.1.117")?)?;
    assert_ne!(plain, on);
    Ok(())
}

#[test]
fn the_knob_needs_a_helper_that_knows_the_verdict_commands() -> TestResult {
    let error = expected_with_notes(&locked(&["Cargo.toml", "lab/Cargo.toml"], "0.1.116")?)
        .err()
        .ok_or("0.1.116 accepted")?
        .to_string();
    assert!(error.contains("pin-helper 0.1.117"), "{error}");
    Ok(())
}

#[test]
fn a_workflow_without_a_plan_job_refuses_the_knob() -> TestResult {
    let error = expected_with_notes(&locked(&["Cargo.toml"], "0.1.117")?)
        .err()
        .ok_or("a single job accepted the knob")?
        .to_string();
    assert!(
        error.contains("needs a workflow with a plan job"),
        "{error}"
    );
    Ok(())
}

#[test]
fn the_plan_job_records_on_a_pull_request_and_reuses_in_the_queue_with_read_only_access()
-> TestResult {
    let all = jobs(&["Cargo.toml", "lab/Cargo.toml"])?;
    let plan = &all["rust-plan"];
    assert_eq!(plan["permissions"]["pull-requests"], "read");
    assert_eq!(plan["permissions"]["actions"], "read");
    assert_eq!(plan["permissions"]["contents"], "read");
    for output in ["reuse", "reuse_run", "reused"] {
        assert_eq!(
            plan["outputs"][output],
            format!("${{{{ steps.verdict_reuse.outputs.{output} }}}}")
        );
    }
    let record = step(plan, "velnor-ci verdict record")?;
    assert!(
        record["run"]
            .as_str()
            .is_some_and(|run| run.ends_with(" verdict record"))
    );
    let when = record["if"].as_str().unwrap_or_default();
    assert!(
        when.contains("github.event_name == 'pull_request'"),
        "{when}"
    );
    assert!(
        when.contains("github.event.pull_request.head.repo.full_name == github.repository"),
        "{when}"
    );
    let upload = step(plan, "Upload the verdict")?;
    assert_eq!(upload["with"]["name"], "velnor-verdict");
    assert_eq!(upload["with"]["retention-days"], 7);
    assert!(
        upload["if"]
            .as_str()
            .unwrap_or_default()
            .contains("steps.verdict_record.outcome == 'success'")
    );
    let reuse = step(plan, "velnor-ci verdict reuse")?;
    assert_eq!(reuse["if"], "${{ github.event_name == 'merge_group' }}");
    assert_eq!(reuse["continue-on-error"], true);
    assert_eq!(
        reuse["env"]["VELNOR_MERGE_GROUP_BASE_SHA"],
        "${{ github.event.merge_group.base_sha }}"
    );
    Ok(())
}

fn root_job() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(jobs(&["Cargo.toml", "lab/Cargo.toml"])?["rust-root"].clone())
}

#[test]
fn format_and_test_steps_wait_for_the_verdict_and_keep_their_own_conditions() -> TestResult {
    let root = root_job()?;
    for (name, id) in [
        ("Format", "rust-root.format-1"),
        ("Test", "rust-root.test-1"),
    ] {
        let when = condition(&root, name)?;
        assert!(
            when.contains(&format!("!{REUSED}{id} ')")),
            "{name}: {when}"
        );
    }
    assert!(
        condition(&root, "Test")?.contains("!cancelled() && steps.restored.outcome == 'success'")
    );
    Ok(())
}

#[test]
fn lint_and_documentation_steps_never_wait_for_the_verdict() -> TestResult {
    let root = root_job()?;
    for name in ["Lint", "Documentation"] {
        assert!(
            !condition(&root, name)?.contains("rust-plan.outputs.reused"),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_reused_test_step_still_builds_its_tests_in_a_twin_that_runs_exactly_then() -> TestResult {
    let root = root_job()?;
    let twin = step(&root, "Test (build only)")?;
    let when = twin["if"].as_str().unwrap_or_default();
    assert!(
        when.contains(&format!("{REUSED}rust-root.test-1 ')"))
            && !when.contains(&format!("!{REUSED}")),
        "{when}"
    );
    let run = twin["run"].as_str().unwrap_or_default();
    assert!(
        run.ends_with(" --no-run") && !run.contains("--no-tests"),
        "{run}"
    );
    assert!(
        step(&root, "Test")?["run"]
            .as_str()
            .is_some_and(|run| !run.contains("--no-run"))
    );
    Ok(())
}

#[test]
fn a_run_that_replanned_everything_keeps_its_checks() -> TestResult {
    let root = root_job()?;
    let skipped = format!("(!{REUSED}rust-root.test-1 ') || steps.replan.outcome == 'success')");
    let built = format!("({REUSED}rust-root.test-1 ') && steps.replan.outcome != 'success')");
    assert!(condition(&root, "Test")?.contains(&skipped));
    assert!(condition(&root, "Test (build only)")?.contains(&built));
    Ok(())
}

#[test]
fn a_reused_lint_job_waits_for_the_plan_and_skips_as_a_whole() -> TestResult {
    let all = jobs(&["Cargo.toml", "lab/Cargo.toml"])?;
    for id in ["lint-jactionlint", "lint-alint"] {
        let when = all[id]["if"].as_str().unwrap_or_default();
        assert!(when.contains(&format!("!{REUSED}{id} ')")), "{id}: {when}");
        assert!(
            all[id]["needs"]
                .as_array()
                .is_some_and(|needs| needs.contains(&Value::from("rust-plan")))
        );
    }
    assert!(
        !all["rust-plan"]["if"]
            .as_str()
            .unwrap_or_default()
            .contains("reused")
    );
    Ok(())
}

#[test]
fn the_reusable_list_names_each_job_and_step_by_the_name_github_reports() -> TestResult {
    let all = jobs(&["Cargo.toml", "lab/Cargo.toml"])?;
    let list = step(&all["rust-plan"], "velnor-ci verdict reuse")?["env"]["VELNOR_REUSABLE"]
        .as_str()
        .ok_or("no list")?
        .to_owned();
    let items: Vec<Value> = serde_json::from_str(&list)?;
    let has = |id: &str, job: &str, step: Option<&str>| {
        items
            .iter()
            .any(|item| item["id"] == id && item["job"] == job && item["step"].as_str() == step)
    };
    assert!(
        has("rust-root.format-1", "rust-root", Some("Format")),
        "{list}"
    );
    assert!(has("rust-root.test-1", "rust-root", Some("Test")), "{list}");
    assert!(has("lint-alint", "Alint", None), "{list}");
    assert!(!list.contains("build only"), "{list}");
    assert!(!list.contains("rust-plan"), "{list}");
    Ok(())
}

#[test]
fn the_gate_spec_names_what_a_verdict_may_stand_in_for() -> TestResult {
    let text = workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], true, "0.1.117")?)?;
    let spec = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("GATE_SPEC: '"))
        .map(|line| line.trim_end_matches('\''))
        .ok_or("no gate spec")?;
    for expected in [
        "--reuse lint-alint",
        "--reuse lint-jactionlint",
        "--reuse-tests rust-root=root:rust-root.test-1",
        "--reuse-tests rust-lab=lab:rust-lab.test-1",
    ] {
        assert!(spec.contains(expected), "{expected} in {spec}");
    }
    assert!(!spec.contains("--reuse rust-root"), "{spec}");
    Ok(())
}

fn with_setup(name: &str, knob: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let config = format!(
        "pr_ci = \"label\"\n\n[[test_setup]]\nroot = \".\"\nname = \"{name}\"\nrun = \"bash scripts/x.sh\"\nphase = \"build\"\n\n{}{LINT}",
        if knob { KNOB } else { "" }
    );
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, "0.1.117")?;
    Ok(repo)
}

#[test]
fn a_setup_step_named_like_a_reusable_step_is_refused() -> TestResult {
    for name in ["Test", "Format"] {
        let error = workflow(&with_setup(name, true)?)
            .err()
            .ok_or("a setup step lent its name to a reusable step")?
            .to_string();
        assert!(
            error.contains(&format!("the step \"{name}\" can be reused")),
            "{name}: {error}"
        );
    }
    Ok(())
}

#[test]
fn a_shared_step_name_is_harmless_without_reuse_and_a_distinct_name_passes() -> TestResult {
    workflow(&with_setup("Test", false)?)?;
    workflow(&with_setup("Prepare fixtures", true)?)?;
    Ok(())
}
