//! P13 performance/verification foundations: scaling, detection, determinism.
//!
//! Owned by the P13 perf builder; wired into `velnor_orchestrator` by the
//! parent with one `mod` line. Fixture and harness helpers ride along via
//! `#[path]` includes, so no other registration is needed.

#[path = "perf_fixtures_p13.rs"]
mod perf_fixtures_p13;

#[path = "perf_harness_p13.rs"]
mod perf_harness_p13;

use velnor_actions_orchestrator::{
    GenerateOptions, OrchestratorError, generate, plan_internal, prepare,
};
use velnor_actions_rust::parse_metadata_json;

use self::perf_fixtures_p13::{malformed_repo, nested_repo, workspace_repo};
use self::perf_harness_p13::{
    commit_two, obligation_task_ids, perf_line, plan_at, plan_two_commits, timed,
};
use crate::impl_common::TestResult;
use crate::impl_common::err_of;

/// Plan wall time plus obligation counts on 1/10/40-crate workspaces.
///
/// 40 stays under the 256 KiB matrix budget; 60 already exceeds it by
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

/// Generate wall time on 1/10/100-crate workspaces (preview dir, no writes).
#[test]
fn generate_scales_with_crate_count() -> TestResult {
    for members in [1_usize, 10, 100] {
        let repo = workspace_repo(members)?;
        let root = repo.path();
        let prep = timed(|| prepare(root));
        let (prep, prep_ms) = (prep.0?, prep.1);
        let out = tempfile::TempDir::new()?;
        let target = out.path().join(format!("gen-{members}"));
        let opts = GenerateOptions {
            output_dir: Some(target),
        };
        let (report, gen_ms) = timed(|| generate(&prep, &opts));
        let report = report?;
        assert!(!report.files_written.is_empty(), "files staged");
        eprintln!(
            "perf: op=generate crates={members} prepare_ms={prep_ms} generate_ms={gen_ms} files={}",
            report.files_written.len()
        );
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
    let from_root = parse_metadata_json(&run("Cargo.toml")?, root, "Cargo.toml")?;
    let from_member = parse_metadata_json(&run("crates/c001/Cargo.toml")?, root, "member")?;
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
