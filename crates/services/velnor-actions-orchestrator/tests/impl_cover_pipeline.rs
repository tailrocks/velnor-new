//! Coverage-pipeline end to end: staged `baseline.json` to merge verdict.
//!
//! The fetch (or plan-artifact) stage writes `baseline.json` beside the
//! downloaded plan; assembly must propagate those exact bytes into the
//! merge request, and the merge must accept covered obligations with
//! zero executed reports. Negatives prove fail-closed: tampered bytes,
//! wrong-commit manifests, and missing evidence all fail planning.

use std::process::Command as StdCommand;

use tempfile::TempDir;
use velnor_actions_contract::digest_b3;
use velnor_actions_contract_workflow::FinalStatus;
use velnor_actions_orchestrator::assemble_merge_request;

use crate::impl_common::{TestResult, plan_for_source_change};
use crate::impl_merge::merge;
use crate::impl_orch_core_cover::covered_plan;

/// Marker proving the child already carries merge-channel env.
const E2E_ENV: &str = "VELNOR_TEST_E2E_MERGE_ENV";

/// Anchor-bearing CI vars scrubbed so fixture manifests match no ambient identity.
const E2E_ANCHORS: [&str; 5] = [
    "GITHUB_REPOSITORY",
    "GITHUB_BASE_REF",
    "GITHUB_REF",
    "GITHUB_WORKFLOW_REF",
    "GITHUB_ACTIONS",
];

/// Same-repo pull-request payload (fork detection needs the flag).
const PR_PAYLOAD: &str = r#"{"pull_request":{"head":{"repo":{"fork":false}}}}"#;

/// Run `inner` in a child carrying the merge-channel env.
///
/// Assembly and merge read their channels from the process
/// environment, which tests must never mutate in-process (parallel
/// hazard; `unsafe` is barred even in tests). The parent re-executes
/// the calling test with anchors scrubbed and the needs/event
/// channels set; the child runs `inner` directly. `test` is the bare
/// test name, unique in the binary.
fn with_merge_channels(test: &str, inner: impl FnOnce() -> TestResult) -> TestResult {
    if std::env::var(E2E_ENV).is_err() {
        let payload_dir = TempDir::new()?;
        let payload = payload_dir.path().join("event.json");
        std::fs::write(&payload, PR_PAYLOAD)?;
        let mut command = StdCommand::new(std::env::current_exe()?);
        command.arg(test).env(E2E_ENV, "1");
        for var in E2E_ANCHORS {
            command.env_remove(var);
        }
        command
            .env("VELNOR_NEEDS_JSON", r#"{"plan":"success"}"#)
            .env("VELNOR_NEEDS_EXPECTED", r#"["plan"]"#)
            .env("GITHUB_EVENT_NAME", "pull_request")
            .env("GITHUB_EVENT_PATH", &payload);
        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

/// Run directory staging one covered plan plus optional baseline bytes.
///
/// No `reports/` tree: covered obligations execute nothing, so the
/// merge must accept the empty report set as proven-by-manifest.
fn stage_run(
    plan_json: &serde_json::Value,
    baseline: Option<&serde_json::Value>,
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let run = TempDir::new()?;
    std::fs::write(run.path().join("plan.json"), plan_json.to_string())?;
    std::fs::write(run.path().join("matrix.json"), r#"{"include":[]}"#)?;
    if let Some(manifest) = baseline {
        std::fs::write(run.path().join("baseline.json"), manifest.to_string())?;
    }
    Ok(run)
}

/// Assembly plus merge verdict for one staged run directory.
fn verdict_for(
    run: &TempDir,
) -> Result<
    (
        serde_json::Value,
        velnor_actions_contract_workflow::FinalReport,
    ),
    Box<dyn std::error::Error>,
> {
    let request: serde_json::Value =
        serde_json::from_str(&assemble_merge_request("local", run.path())?)?;
    let report = merge(&request)?;
    Ok((request, report))
}

#[test]
fn staged_baseline_assembles_and_merges_passed() -> TestResult {
    with_merge_channels("staged_baseline_assembles_and_merges_passed", || {
        let (_repo, plan) = plan_for_source_change()?;
        assert!(!plan.task_ids.is_empty(), "fixture must select work");
        let (plan_json, manifest) = covered_plan(&plan)?;
        let run = stage_run(&plan_json, Some(&manifest))?;
        let (request, report) = verdict_for(&run)?;
        assert!(
            request["assembly_errors"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "clean pipeline stages no gaps: {:?}",
            request["assembly_errors"]
        );
        assert_eq!(request["baseline_manifest"], manifest);
        assert_eq!(report.status, FinalStatus::Passed);
        assert_eq!(report.counts.covered as usize, plan.task_ids.len());
        assert_eq!(report.counts.not_run, 0);
        Ok(())
    })
}

#[test]
fn tampered_staged_baseline_fails_closed() -> TestResult {
    with_merge_channels("tampered_staged_baseline_fails_closed", || {
        let (_repo, plan) = plan_for_source_change()?;
        let (plan_json, mut manifest) = covered_plan(&plan)?;
        manifest["tasks"][0]["task_digest"] = serde_json::json!(digest_b3(b"tampered-task"));
        let run = stage_run(&plan_json, Some(&manifest))?;
        let (request, report) = verdict_for(&run)?;
        assert_eq!(
            request["baseline_manifest"], manifest,
            "assembly propagates bytes verbatim; the merge judges content"
        );
        assert_eq!(report.status, FinalStatus::PlanningFailed);
        Ok(())
    })
}

#[test]
fn mismatched_staged_baseline_fails_closed() -> TestResult {
    with_merge_channels("mismatched_staged_baseline_fails_closed", || {
        let (_repo, plan) = plan_for_source_change()?;
        let (plan_json, mut manifest) = covered_plan(&plan)?;
        manifest["source_commit"] = serde_json::json!("2".repeat(40));
        let run = stage_run(&plan_json, Some(&manifest))?;
        let (_, report) = verdict_for(&run)?;
        assert_eq!(report.status, FinalStatus::PlanningFailed);
        Ok(())
    })
}

#[test]
fn missing_staged_baseline_fails_closed() -> TestResult {
    with_merge_channels("missing_staged_baseline_fails_closed", || {
        let (_repo, plan) = plan_for_source_change()?;
        let (plan_json, _) = covered_plan(&plan)?;
        let run = stage_run(&plan_json, None)?;
        let (request, report) = verdict_for(&run)?;
        assert!(
            request["assembly_errors"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "baseline stays optional at assembly: {:?}",
            request["assembly_errors"]
        );
        assert_eq!(report.status, FinalStatus::PlanningFailed);
        Ok(())
    })
}
