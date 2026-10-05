//! T24 tofu synthetic scaling: plan/prepare/generate across
//! 1/10/100-root fixtures (the P13 `workspace_repo` pattern for tofu).
//!
//! Walls are local observations via the P13 `timed` harness
//! (`perf:` lines), not hosted deployment performance.

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::TestResult;
use crate::impl_perf_p13::perf_harness_p13::{perf_line, timed};
use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::{
    commit_two_tofu, plan_at_event, tofu_repo,
};

/// Plan wall plus obligation counts on 1/10/40-root fixtures.
///
/// 40 stays under the 512 KiB matrix-artifact budget (the P13 40-crate
/// precedent); wider plans fail closed on the budget instead.
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

/// Generate wall time on 1/10/100 roots (preview dir, no writes).
#[test]
fn tofu_generate_scales_with_root_count() -> TestResult {
    for roots in [1_usize, 10, 100] {
        let repo = tofu_repo(roots)?;
        let root = repo.path();
        let (prep, prep_ms) = timed(|| prepare(root));
        let prep = prep?;
        let out = tempfile::TempDir::new()?;
        let target = out.path().join(format!("gen-{roots}"));
        let opts = GenerateOptions {
            output_dir: Some(target),
        };
        let (report, gen_ms) = timed(|| generate(&prep, &opts));
        let report = report?;
        assert!(!report.files_written.is_empty(), "files staged");
        eprintln!(
            "perf: op=generate roots={roots} prepare_ms={prep_ms} generate_ms={gen_ms} files={}",
            report.files_written.len()
        );
    }
    Ok(())
}
