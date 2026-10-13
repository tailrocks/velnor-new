//! Validator-failure negatives against the rendered needs inventory.
//!
//! Earlier negatives hand-set a two-job inventory; these tests render a
//! real Velnor-policy workflow, extract `VELNOR_NEEDS_EXPECTED` from the
//! emitted YAML, and fail one global validator against passing crates.

use std::fs;

use velnor_actions_contract::{FinalReport, FinalStatus, JobConclusion, Plan};
use velnor_actions_orchestrator::{
    GenerationPreparation, merge_passed, plan_internal, prepare, render_staged_tree,
};

use crate::impl_common::{
    TestResult, git, git_line, make_repo, passing_reports, without_ambient_identity,
};
use crate::impl_orch_core::{merge, merge_request};

/// Velnor-policy config: repository validators join the workflow.
fn velnor_config() -> String {
    "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n"
        .to_owned()
}

/// Fixture accepted by the Velnor-repository identity check.
fn make_velnor_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(&velnor_config())?;
    let git_config = repo.path().join(".git/config");
    let mut text = fs::read_to_string(&git_config)?;
    text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
    fs::write(&git_config, text)?;
    Ok(repo)
}

/// Rendered `ci.yml` text for one preparation.
fn rendered_workflow(prep: &GenerationPreparation) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(prep)?;
    tree.get(".github/workflows/ci.yml")
        .map(str::to_owned)
        .ok_or_else(|| {
            Box::new(std::io::Error::other("missing workflow")) as Box<dyn std::error::Error>
        })
}

/// `VELNOR_NEEDS_EXPECTED` inventory parsed from rendered YAML.
///
/// Mirrors assembly (`parse_expected`): the matrix-driver job needs no
/// conclusion because per-leg reports prove it, so it leaves the
/// required inventory.
fn rendered_needs_inventory(yaml: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let line = yaml
        .lines()
        .find(|line| line.contains("VELNOR_NEEDS_EXPECTED:"))
        .ok_or("missing needs-expected line")?;
    let scalar = line
        .split_once("VELNOR_NEEDS_EXPECTED:")
        .ok_or("needs-expected shape")?
        .1
        .trim();
    // Double-quoted YAML scalar: strip the quotes, unescape the JSON.
    let unquoted = scalar
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .ok_or("needs-expected quoting")?;
    let raw = unquoted.replace("\\\"", "\"");
    let mut inventory: Vec<String> = serde_json::from_str(&raw)?;
    inventory.retain(|id| id != velnor_actions_workflow_renderer::render::TASK_JOB_ID);
    inventory.sort();
    Ok(inventory)
}

/// Merge verdict over passing crates with one validator at `conclusion`.
fn merge_with_validator(
    plan: &Plan,
    inventory: &[String],
    failing: &str,
    conclusion: &str,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    let reports = passing_reports(plan)?;
    let jobs: Vec<serde_json::Value> = inventory
        .iter()
        .map(|id| {
            let result = if id == failing { conclusion } else { "success" };
            serde_json::json!({"job_id": id, "conclusion": result})
        })
        .collect();
    let plan_value = serde_json::to_value(plan)?;
    let mut request = merge_request(
        &plan_value,
        &plan_value["matrix"].clone(),
        &serde_json::to_value(&reports)?,
        &serde_json::Value::Array(jobs),
    );
    request["required_job_ids"] = serde_json::to_value(inventory)?;
    merge(&request)
}

/// Assert the verdict blames `failing` with `conclusion`.
fn assert_validator_blamed(
    report: &FinalReport,
    failing: &str,
    conclusion: JobConclusion,
    status: FinalStatus,
) {
    assert_eq!(report.status, status, "{failing} verdict");
    let blamed = report
        .required_job_results
        .iter()
        .find(|job| job.job_id == failing);
    assert_eq!(
        blamed.map(|job| job.conclusion),
        Some(conclusion),
        "{failing} blamed: {:?}",
        report.required_job_results
    );
}

#[test]
fn rendered_inventory_validator_failure_fails_required() -> TestResult {
    without_ambient_identity(
        "rendered_inventory_validator_failure_fails_required",
        || {
            let repo = make_velnor_repo()?;
            let root = repo.path();
            git(&["add", "."], root)?;
            git(&["commit", "-m", "one"], root)?;
            fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
            git(&["add", "."], root)?;
            git(&["commit", "-m", "two"], root)?;
            let base = git_line(&["rev-parse", "HEAD~1"], root)?;
            let head = git_line(&["rev-parse", "HEAD"], root)?;

            let yaml = rendered_workflow(&prepare(root)?)?;
            let inventory = rendered_needs_inventory(&yaml)?;
            assert!(
                inventory.contains(&"plan".to_owned()),
                "plan gates: {inventory:?}"
            );
            assert!(
                inventory.contains(&"zizmor".to_owned()),
                "global validator gates: {inventory:?}"
            );
            assert!(
                inventory.len() > 2,
                "rendered set, not a hand-set pair: {inventory:?}"
            );

            let request = serde_json::json!({
                "schema": 1, "run_key": "local", "base": base, "head": head,
                "event": "pull_request", "root": root.display().to_string(),
            });
            let response = plan_internal(&request.to_string())?;
            let plan: Plan = serde_json::from_value(
                serde_json::from_str::<serde_json::Value>(&response)?["plan"].clone(),
            )?;
            for (conclusion, verdict, blame) in [
                ("failure", FinalStatus::Failed, JobConclusion::Failure),
                ("skipped", FinalStatus::NotRun, JobConclusion::Skipped),
                (
                    "cancelled",
                    FinalStatus::Cancelled,
                    JobConclusion::Cancelled,
                ),
            ] {
                let report = merge_with_validator(&plan, &inventory, "zizmor", conclusion)?;
                assert_validator_blamed(&report, "zizmor", blame, verdict);
            }
            Ok(())
        },
    )
}

#[test]
fn rendered_inventory_validator_failure_fails_without_work() -> TestResult {
    without_ambient_identity(
        "rendered_inventory_validator_failure_fails_without_work",
        || {
            let repo = make_velnor_repo()?;
            let root = repo.path();
            fs::remove_file(root.join("Cargo.toml"))?;
            fs::remove_dir_all(root.join("src"))?;
            fs::write(root.join("README.md"), "no manifests here\n")?;
            git(&["add", "."], root)?;
            git(&["commit", "-m", "one"], root)?;
            let head = git_line(&["rev-parse", "HEAD"], root)?;

            let request = serde_json::json!({
                "schema": 1, "run_key": "local", "base": None::<String>, "head": head,
                "event": "push", "root": root.display().to_string(),
            });
            let response = plan_internal(&request.to_string())?;
            let plan: Plan = serde_json::from_value(
                serde_json::from_str::<serde_json::Value>(&response)?["plan"].clone(),
            )?;
            assert!(plan.task_ids.is_empty(), "no inventory, no work");

            let yaml = rendered_workflow(&prepare(root)?)?;
            let inventory = rendered_needs_inventory(&yaml)?;
            assert!(
                inventory.contains(&"zizmor".to_owned()),
                "global validator gates: {inventory:?}"
            );

            // A failing validator fails Required even with no crate work.
            let report = merge_with_validator(&plan, &inventory, "zizmor", "failure")?;
            assert_validator_blamed(
                &report,
                "zizmor",
                JobConclusion::Failure,
                FinalStatus::Failed,
            );
            // Clean validators with no work prove nothing: gate stays red.
            let clean = merge_with_validator(&plan, &inventory, "zizmor", "success")?;
            assert_eq!(clean.status, FinalStatus::NoWork);
            assert!(
                !merge_passed(&serde_json::to_string(&clean)?)?,
                "no work proves nothing"
            );
            Ok(())
        },
    )
}
