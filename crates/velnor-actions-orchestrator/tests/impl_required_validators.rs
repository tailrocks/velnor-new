//! Validator-failure negatives against the rendered needs inventory digest.
//!
//! Earlier negatives hand-set a two-job inventory; these tests render a
//! real Velnor-policy workflow, extract `VELNOR_NEEDS_EXPECTED` from the
//! emitted YAML, confirm it binds the Required needs set, and fail one
//! global validator against passing crates.

use std::fs;

use velnor_actions_contract::{FinalStatus, JobConclusion, Plan};
use velnor_actions_orchestrator::{merge_passed, plan_internal, prepare};

use crate::impl_common::{TestResult, git, git_line, without_ambient_identity};
use crate::impl_required_validators_support::*;

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

/// Every Required-gating validator job ID in the rendered workflow.
fn required_validators() -> [&'static str; 6] {
    [
        "plan",
        "actionlint",
        "alint",
        "cargo-deny",
        "cargo-machete",
        "zizmor",
    ]
}

/// Drive one validator through every negative conclusion.
///
/// Each case runs against passing crate reports with a nonempty
/// Required inventory; the verdict must fail and blame the validator.
fn assert_validator_negatives(
    plan: &Plan,
    inventory: &[String],
    validator: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for (conclusion, verdict, blame) in [
        ("failure", FinalStatus::Failed, JobConclusion::Failure),
        (
            "cancelled",
            FinalStatus::Cancelled,
            JobConclusion::Cancelled,
        ),
        ("skipped", FinalStatus::NotRun, JobConclusion::Skipped),
    ] {
        let report = merge_with_validator(plan, inventory, validator, conclusion)?;
        assert!(
            !report.required_job_results.is_empty(),
            "{validator} keeps a nonempty Required"
        );
        assert_validator_blamed(&report, validator, blame, verdict);
    }
    let report = merge_omitting_validator(plan, inventory, validator)?;
    assert!(
        !report.required_job_results.is_empty(),
        "{validator} keeps a nonempty Required"
    );
    assert_validator_blamed(
        &report,
        validator,
        JobConclusion::Missing,
        FinalStatus::PlanningFailed,
    );
    Ok(())
}

#[test]
fn rendered_inventory_each_validator_fails_required() -> TestResult {
    without_ambient_identity("rendered_inventory_each_validator_fails_required", || {
        let (repo, base, head) = velnor_repo_with_work()?;
        let root = repo.path();

        let yaml = rendered_workflow(&prepare(root)?)?;
        let inventory = rendered_needs_inventory(&yaml)?;
        for validator in required_validators() {
            assert!(
                inventory.contains(&validator.to_owned()),
                "{validator} gates Required: {inventory:?}"
            );
        }

        let request = serde_json::json!({
            "schema": 1, "run_key": "local", "base": base, "head": head,
            "event": "pull_request", "root": root.display().to_string(),
        });
        let response = plan_internal(&request.to_string())?;
        let plan: Plan = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(&response)?["plan"].clone(),
        )?;
        for validator in required_validators() {
            assert_validator_negatives(&plan, &inventory, validator)?;
        }
        Ok(())
    })
}

/// Velnor-policy repo with one source change plus its base/head revisions.
fn velnor_repo_with_work() -> Result<(tempfile::TempDir, String, String), Box<dyn std::error::Error>>
{
    let repo = make_velnor_repo()?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    Ok((repo, base, head))
}

#[test]
fn rendered_inventory_all_success_passes_required() -> TestResult {
    without_ambient_identity("rendered_inventory_all_success_passes_required", || {
        let (repo, base, head) = velnor_repo_with_work()?;
        let root = repo.path();

        let yaml = rendered_workflow(&prepare(root)?)?;
        let inventory = rendered_needs_inventory(&yaml)?;
        assert!(
            !inventory.contains(&"publish-baseline".to_owned()),
            "downstream publish never gates Required: {inventory:?}"
        );
        assert!(
            inventory.contains(&"plan".to_owned()) && inventory.contains(&"zizmor".to_owned()),
            "upstream plan and validators still gate: {inventory:?}"
        );

        let request = serde_json::json!({
            "schema": 1, "run_key": "local", "base": base, "head": head,
            "event": "pull_request", "root": root.display().to_string(),
        });
        let response = plan_internal(&request.to_string())?;
        let plan: Plan = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(&response)?["plan"].clone(),
        )?;
        // Every validator succeeds (a skipped-by-condition downstream
        // publish contributes no conclusion because it is not
        // expected): Required must be satisfiable, not stuck red.
        let report = merge_with_validator(&plan, &inventory, "zizmor", "success")?;
        assert_eq!(report.status, FinalStatus::Passed, "clean tree passes");
        assert!(
            merge_passed(&serde_json::to_string(&report)?)?,
            "clean tree reports passed"
        );
        Ok(())
    })
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
            // Clean validators with no work passes the gate.
            let clean = merge_with_validator(&plan, &inventory, "zizmor", "success")?;
            assert_eq!(clean.status, FinalStatus::NoWork);
            assert!(
                merge_passed(&serde_json::to_string(&clean)?)?,
                "clean validators with no work passes"
            );
            Ok(())
        },
    )
}
