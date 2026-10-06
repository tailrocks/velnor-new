//! P13 performance/verification foundations: scaling, detection, determinism.
//!
//! Owned by the P13 perf builder; wired into `velnor_orchestrator` by the
//! parent with one `mod` line. Fixture and harness helpers ride along via
//! `#[path]` includes, so no other registration is needed.

#[path = "perf_fixtures_p13.rs"]
pub(crate) mod perf_fixtures_p13;

#[path = "perf_harness_p13.rs"]
pub(crate) mod perf_harness_p13;

use velnor_actions_orchestrator::{
    GenerateOptions, OrchestratorError, PlanOutputMode, generate, plan_internal, plan_outputs,
    prepare,
};
use velnor_actions_rust::parse_metadata_json;
use velnor_actions_workflow_renderer::MAX_WORKFLOW_BYTES;

use self::perf_fixtures_p13::{malformed_repo, nested_path_dep_repo, nested_repo, workspace_repo};
use self::perf_harness_p13::{
    commit_two, obligation_task_ids, perf_line, plan_at, plan_two_commits, timed,
};
use crate::impl_common::err_of;
use crate::impl_common::{TestResult, git, git_line};

/// Plan wall time plus obligation counts on 1/10/40-crate workspaces.
///
/// 40 stays under the 512 KiB matrix-artifact budget; 60 exceeds it by
/// design (`matrix_budget_enforced_never_truncated` pins that ceiling).
#[test]
fn plan_scales_with_crate_count() -> TestResult {
    for members in [1_usize, 10, 40] {
        let repo = workspace_repo(members)?;
        let root = repo.path();
        let (outcome, wall_ms) = timed(|| plan_two_commits(root, "src/lib.rs"));
        let (plan, _) = outcome?;
        assert!(!plan.obligations.is_empty(), "obligations exist");
        perf_line("plan", members, wall_ms, &plan);
    }
    Ok(())
}

/// A full 44-crate dynamic plan fits both independent platform budgets.
#[test]
fn full_44_crate_matrix_fits_artifact_and_job_output_budgets() -> TestResult {
    let repo = workspace_repo(43)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "initial"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": serde_json::Value::Null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: velnor_actions_contract::Plan = serde_json::from_value(value["plan"].clone())?;
    assert_eq!(plan.packages.len(), 44, "root package plus 43 members");
    assert!(!plan.matrix.include.is_empty(), "dynamic matrix has work");
    assert!(
        plan.matrix.include.len() <= 256,
        "within Actions matrix job cap"
    );
    let outputs = plan_outputs(&response, PlanOutputMode::DynamicMatrix)?;
    assert!(outputs.matrix.len() <= 512 * 1024, "matrix artifact budget");
    assert!(
        outputs.job_outputs_utf16_bytes <= 900_000,
        "aggregate UTF-16 output budget"
    );
    eprintln!(
        "matrix-acceptance: crates={} matrix_entries={} matrix_bytes={} job_outputs_utf16_bytes={}",
        plan.packages.len(),
        plan.matrix.include.len(),
        outputs.matrix.len(),
        outputs.job_outputs_utf16_bytes,
    );
    Ok(())
}

/// Prepare (discovery plus graph construction) wall on 1/10/100 crates.
/// This profiles the optimized inventory path end to end; full `plan` at
/// 100 crates cannot complete (matrix budget, next test).
#[test]
fn prepare_scales_to_100_crates() -> TestResult {
    for members in [1_usize, 10, 100] {
        let repo = workspace_repo(members)?;
        let root = repo.path();
        let (prep, wall_ms) = timed(|| prepare(root));
        let prep = prep?;
        assert_eq!(prep.discovery.workspaces.len(), 1);
        assert_eq!(
            prep.discovery.workspaces[0].record.packages.len(),
            members + 1
        );
        eprintln!(
            "perf: op=prepare crates={members} wall_ms={wall_ms} packages={}",
            members + 1
        );
    }
    Ok(())
}

/// A 100-crate full plan fails closed on the matrix budget, never truncates.
#[test]
fn plan_100_crates_reports_matrix_budget() -> TestResult {
    let repo = workspace_repo(100)?;
    let root = repo.path();
    let (base, head) = commit_two(root, "src/lib.rs")?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let err = err_of(plan_internal(&request.to_string()), "100-crate plan")?;
    assert!(
        err.to_string().contains("matrix_budget_exceeded"),
        "got {err}"
    );
    Ok(())
}

/// Measured generation stays within the fixed byte cap for 1/10/29/30/40 members.
///
/// The 500,000-byte workflow contract, not a crate-count promise, is the
/// limit. The 60/100-member fixtures are retained as measured over-cap failures.
#[test]
fn generate_scales_to_workflow_file_limit_then_fails_closed() -> TestResult {
    for members in [1_usize, 10, 29, 30, 40, 60, 100] {
        let repo = workspace_repo(members)?;
        let root = repo.path();
        let prep = timed(|| prepare(root));
        let (prep, prep_ms) = (prep.0?, prep.1);
        let out = tempfile::TempDir::new()?;
        let target = out.path().join(format!("gen-{members}"));
        let opts = GenerateOptions {
            output_dir: Some(target.clone()),
        };
        let (result, gen_ms) = timed(|| generate(&prep, &opts));
        if members >= 60 {
            let Err(error) = result else {
                return Err(std::io::Error::other(format!(
                    "{members}-member workflow unexpectedly fit the contract"
                ))
                .into());
            };
            let diagnostic = error.to_string();
            let actual_bytes = diagnostic
                .strip_prefix(
                    "render: invalid workflow: workflow_too_large:.github/workflows/ci.yml:",
                )
                .and_then(|detail| detail.strip_suffix(&format!(":{MAX_WORKFLOW_BYTES}")))
                .ok_or_else(|| format!("unexpected workflow-size diagnostic: {diagnostic}"))?
                .parse::<usize>()?;
            assert!(
                actual_bytes > MAX_WORKFLOW_BYTES,
                "diagnostic reported {actual_bytes} bytes"
            );
            assert!(
                !target.exists(),
                "failed preview generation left a partial output tree"
            );
            eprintln!(
                "perf: op=generate crates={members} prepare_ms={prep_ms} generate_ms={gen_ms} failed_closed_bytes={actual_bytes} limit_bytes={MAX_WORKFLOW_BYTES}"
            );
        } else {
            let report = result?;
            assert!(!report.files_written.is_empty(), "files staged");
            let workflow = std::fs::read(target.join(".github/workflows/ci.yml"))?;
            assert!(
                workflow.len() <= MAX_WORKFLOW_BYTES,
                "{members} members generated {} bytes, limit is {MAX_WORKFLOW_BYTES}",
                workflow.len()
            );
            eprintln!(
                "perf: op=generate crates={members} prepare_ms={prep_ms} generate_ms={gen_ms} files={} ci_bytes={}",
                report.files_written.len(),
                workflow.len()
            );
        }
    }
    Ok(())
}

/// Nested and independent workspaces keep their own inventories.
#[test]
fn detection_preserved_for_nested_and_independent() -> TestResult {
    let repo = nested_repo()?;
    let root = repo.path();
    let prep = prepare(root)?;
    let mut roots: Vec<String> = prep
        .discovery
        .workspaces
        .iter()
        .map(|w| w.record.workspace_root.clone())
        .collect();
    roots.sort();
    assert_eq!(roots, ["", "nested", "tools/tool"]);
    let (plan, _) = plan_two_commits(root, "src/lib.rs")?;
    assert!(!plan.obligations.is_empty(), "obligations exist");
    Ok(())
}

/// A nested workspace with a path dependency on its parent skips the
/// cross-workspace edge instead of failing discovery (termpane shape).
#[test]
fn nested_cross_workspace_path_dep_skips_not_fails() -> TestResult {
    let repo = nested_path_dep_repo()?;
    let root = repo.path();
    let prep = prepare(root)?;
    let mut roots: Vec<String> = prep
        .discovery
        .workspaces
        .iter()
        .map(|w| w.record.workspace_root.clone())
        .collect();
    roots.sort();
    assert_eq!(roots, ["", "fuzz"]);
    for workspace in &prep.discovery.workspaces {
        if workspace.record.workspace_root == "fuzz" {
            assert_eq!(workspace.record.members.len(), 1);
            assert_eq!(workspace.record.skipped_edges.len(), 1);
        } else {
            assert!(workspace.record.skipped_edges.is_empty());
        }
    }
    Ok(())
}

/// Member-manifest metadata parses to the same record as root metadata.
///
/// Proves the reuse premise on real `cargo metadata` output: the only
/// differing field (`workspace_default_members`) is never parsed.
#[test]
fn member_metadata_parses_to_same_record() -> TestResult {
    let repo = workspace_repo(3)?;
    let root = repo.path();
    let run = |manifest: &str| -> Result<String, Box<dyn std::error::Error>> {
        let output = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
                "--manifest-path",
            ])
            .arg(root.join(manifest))
            .output()?;
        if !output.status.success() {
            return Err(format!("cargo metadata failed for {manifest}").into());
        }
        Ok(String::from_utf8(output.stdout)?)
    };
    let known = std::collections::BTreeSet::new();
    let from_root = parse_metadata_json(&run("Cargo.toml")?, root, "Cargo.toml", &known)?;
    let from_member = parse_metadata_json(&run("crates/c001/Cargo.toml")?, root, "member", &known)?;
    assert_eq!(from_root, from_member, "parsed records identical");
    Ok(())
}

/// Malformed manifests still fail the plan with a detection error.
#[test]
fn malformed_manifest_still_fails_plan() -> TestResult {
    let repo = malformed_repo()?;
    let err = err_of(prepare(repo.path()), "malformed manifest")?;
    assert!(
        matches!(err, OrchestratorError::Detection { .. }),
        "got {err:?}"
    );
    Ok(())
}

/// Same request planned twice yields the same obligation set and JSON.
#[test]
fn plan_obligation_set_is_deterministic() -> TestResult {
    let repo = workspace_repo(10)?;
    let root = repo.path();
    let (base, head) = commit_two(root, "src/lib.rs")?;
    let (first, first_raw) = plan_at(root, &base, &head)?;
    let (second, second_raw) = plan_at(root, &base, &head)?;
    assert_eq!(obligation_task_ids(&first), obligation_task_ids(&second));
    let first_json: serde_json::Value = serde_json::from_str(&first_raw)?;
    let second_json: serde_json::Value = serde_json::from_str(&second_raw)?;
    assert_eq!(first_json, second_json, "byte-identical plan JSON");
    Ok(())
}
