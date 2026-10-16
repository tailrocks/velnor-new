//! T24 tofu synthetic scaling: plan/prepare/generate across
//! 1/10/100-root fixtures (the P13 `workspace_repo` pattern for tofu).
//!
//! Walls are local observations via the P13 `timed` harness
//! (`perf:` lines), not hosted deployment performance.

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};
use velnor_actions_workflow_renderer::MAX_WORKFLOW_BYTES;

use crate::impl_common::TestResult;
use crate::impl_perf_p13::perf_harness_p13::{perf_line, timed};
use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::{
    commit_two_tofu, plan_at_event, tofu_repo,
};

/// Plan wall plus obligation counts on 1/10/40-root fixtures.
///
/// 40 and wider plans stay inside the matrix-artifact budget. An oversize
/// matrix still fails closed and is never truncated.
#[test]
fn tofu_plan_scales_with_root_count() -> TestResult {
    for roots in [1_usize, 10, 40] {
        let repo = tofu_repo(roots)?;
        let root = repo.path();
        let (base, head) = commit_two_tofu(root, "README.md", "more docs\n")?;
        let (outcome, wall_ms) = timed(|| plan_at_event(root, &base, &head, "pull_request"));
        let (plan, _) = outcome?;
        assert_eq!(plan.obligations.len(), 3 * roots, "{roots} triples");
        perf_line("plan", roots, wall_ms, &plan);
    }
    Ok(())
}

/// Prepare (discovery plus graph construction) wall on 1/10/100 roots.
#[test]
fn tofu_prepare_scales_to_100_roots() -> TestResult {
    for roots in [1_usize, 10, 100] {
        let repo = tofu_repo(roots)?;
        let root = repo.path();
        let (prep, wall_ms) = timed(|| prepare(root));
        let prep = prep?;
        let tofu: Vec<_> = prep
            .discovery
            .proposals
            .iter()
            .filter(|task| task.stack_id == "tofu")
            .collect();
        assert_eq!(tofu.len(), 3 * roots, "{roots} triples proposed");
        eprintln!(
            "perf: op=prepare roots={roots} wall_ms={wall_ms} proposals={}",
            3 * roots
        );
    }
    Ok(())
}

/// Generate measured root counts inside the 500 KB workflow file.
///
/// Wide root sets keep one job per root by moving obligation commands into
/// local composite actions. The shared runtime identity helper is emitted
/// once; each typed job retains its own lane identity, tool payload, and
/// provider restore operations.
#[test]
fn tofu_generate_scales_with_root_count() -> TestResult {
    for roots in [1_usize, 10, 31, 32, 40, 60, 100] {
        let repo = tofu_repo(roots)?;
        let root = repo.path();
        let (prep, prep_ms) = timed(|| prepare(root));
        let prep = prep?;
        let out = tempfile::TempDir::new()?;
        let target = out.path().join(format!("gen-{roots}"));
        let opts = GenerateOptions {
            output_dir: Some(target.clone()),
        };
        let (result, gen_ms) = timed(|| generate(&prep, &opts));
        let report = result?;
        assert!(!report.files_written.is_empty(), "files staged");
        let workflow = std::fs::read(target.join(".github/workflows/ci.yml"))?;
        assert!(
            workflow.len() <= MAX_WORKFLOW_BYTES,
            "{roots} roots generated {} bytes, limit is {MAX_WORKFLOW_BYTES}",
            workflow.len()
        );
        let text = String::from_utf8(workflow)?;
        let jobs = text
            .lines()
            .filter(|line| line.starts_with("  tofu-"))
            .count();
        assert!(jobs >= roots, "{roots} roots scheduled {jobs} tofu jobs");
        eprintln!(
            "perf: op=generate roots={roots} prepare_ms={prep_ms} generate_ms={gen_ms} files={} ci_bytes={}",
            report.files_written.len(),
            text.len()
        );
    }
    Ok(())
}
