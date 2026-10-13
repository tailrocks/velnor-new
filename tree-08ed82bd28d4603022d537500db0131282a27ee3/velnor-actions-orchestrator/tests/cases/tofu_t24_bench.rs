//! T24 tofu benchmark matrix: cold, warm, docs, root, module, lock,
//! mixed, and fork cases over fixed-shape fixtures.
//!
//! Every case emits one machine-readable `bench:` line via the P13
//! harness (`timed`/`timed_rss`/`bench_line`/`obligation_digest`;
//! nextest-only, no criterion). `metadata_ms` is a direct-`git
//! ls-files` lower-bound proxy for the index-subprocess cost inside
//! `plan`. Queue/transfer are local no-ops; CI values need hosted runs.

use std::fs;

use velnor_actions_contract_workflow::Plan;

use crate::cases::tofu_t24_gates::tofu_perf_fixtures_t24::{
    commit_two_tofu, index_baseline_ms, mixed_repo, plan_at_event, tofu_repo, tofu_repo_with_lock,
    tofu_repo_with_module,
};
use crate::impl_perf_p13::perf_harness_p13::{
    BenchSample, bench_line, obligation_digest, timed, timed_rss,
};
use crate::support::{TestResult, git, git_line};

/// Fixture width for the fixed-shape matrix cases (30 obligations).
const BENCH_ROOTS: usize = 10;

/// HCL-safe second-commit touch for `.tf` files.
const TF_TOUCH: &str = "variable \"bump\" {}\n";

/// Emit one `bench:` line for a planned sample.
fn report(sample: &BenchSample<'_>) {
    bench_line(sample);
}

/// Reasons of every obligation as `(task_id, reason)` pairs.
fn reasons(plan: &Plan) -> Vec<(&str, &str)> {
    plan.obligations
        .iter()
        .map(|ob| (ob.task_id.as_str(), ob.reason.as_str()))
        .collect()
}

/// Cold plan on a fixture never planned before.
#[test]
fn bench_tofu_cold_first_plan() -> TestResult {
    let (repo, setup_ms) = timed(|| tofu_repo(BENCH_ROOTS));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "stacks/r003/main.tf", TF_TOUCH);
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert_eq!(plan.obligations.len(), 3 * BENCH_ROOTS);
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-cold",
        crates: BENCH_ROOTS,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "first-plan",
    });
    Ok(())
}

/// Warm restore: second plan on the same commits keeps the set.
#[test]
fn bench_tofu_warm_second_plan() -> TestResult {
    let (repo, setup_ms) = timed(|| tofu_repo(BENCH_ROOTS));
    let repo = repo?;
    let root = repo.path();
    let (base, head) = commit_two_tofu(root, "stacks/r003/main.tf", TF_TOUCH)?;
    let (first, cold_ms, _) = timed_rss(|| plan_at_event(root, &base, &head, "pull_request"));
    let (first, _) = first?;
    let (second, warm_ms, rss_kb) = timed_rss(|| plan_at_event(root, &base, &head, "pull_request"));
    let (second, _) = second?;
    assert_eq!(
        obligation_digest(&first),
        obligation_digest(&second),
        "warm plan keeps the obligation set"
    );
    let metadata_ms = index_baseline_ms(root)?;
    let note = format!("cold_ms={cold_ms}");
    report(&BenchSample {
        case: "tofu-warm",
        crates: BENCH_ROOTS,
        setup_ms,
        plan_ms: warm_ms,
        metadata_ms,
        rss_kb,
        plan: &second,
        note: &note,
    });
    Ok(())
}

/// Docs-only touch: full universe planned, zero affected.
#[test]
fn bench_tofu_docs_only() -> TestResult {
    let (repo, setup_ms) = timed(|| tofu_repo(BENCH_ROOTS));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "README.md", "more docs\n");
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert_eq!(plan.obligations.len(), 3 * BENCH_ROOTS);
    assert!(
        reasons(&plan)
            .iter()
            .all(|(_, reason)| *reason != "affected_by_change"),
        "docs touch affects nothing"
    );
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-docs",
        crates: BENCH_ROOTS,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "touch=README.md",
    });
    Ok(())
}

/// Root change: exactly the touched root's triple is affected.
#[test]
fn bench_tofu_root_change() -> TestResult {
    let (repo, setup_ms) = timed(|| tofu_repo(BENCH_ROOTS));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "stacks/r003/main.tf", TF_TOUCH);
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    let affected: Vec<(&str, &str)> = reasons(&plan)
        .into_iter()
        .filter(|(_, reason)| *reason == "affected_by_change")
        .collect();
    assert_eq!(affected.len(), 3, "touched triple only: {affected:?}");
    assert!(
        affected
            .iter()
            .all(|(task, _)| task.contains("stacks/r003")),
        "{affected:?}"
    );
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-root",
        crates: BENCH_ROOTS,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "touch=r003",
    });
    Ok(())
}

/// Shared-module change: the calling root's triple is affected.
#[test]
fn bench_tofu_shared_module_change() -> TestResult {
    let (repo, setup_ms) = timed(tofu_repo_with_module);
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "mods/shared/main.tf", TF_TOUCH);
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert_eq!(plan.obligations.len(), 3, "one calling root");
    assert!(
        reasons(&plan)
            .iter()
            .all(|(_, reason)| *reason == "affected_by_change"),
        "module change selects the caller"
    );
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-module",
        crates: 1,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "touch=shared-module",
    });
    Ok(())
}

/// Lock change: the lockful root's triple is affected.
#[test]
fn bench_tofu_lock_change() -> TestResult {
    let (repo, setup_ms) = timed(tofu_repo_with_lock);
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "stacks/a/.terraform.lock.hcl", "# bump\n");
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert_eq!(plan.obligations.len(), 3, "one lockful root");
    assert!(
        reasons(&plan)
            .iter()
            .all(|(_, reason)| *reason == "affected_by_change"),
        "lock change selects the root"
    );
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-lock",
        crates: 1,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "touch=lockfile",
    });
    Ok(())
}

/// Mixed change: a Rust touch affects Rust only, tofu stays planned.
#[test]
fn bench_tofu_mixed_rust_change() -> TestResult {
    let (repo, setup_ms) = timed(|| mixed_repo(2));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "src/lib.rs", "pub fn g() {}\n");
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert!(
        plan.obligations.len() > 6,
        "rust plus two tofu triples: {}",
        plan.obligations.len()
    );
    assert!(
        reasons(&plan)
            .iter()
            .any(|(task, reason)| !task.starts_with("stack/tofu/")
                && *reason == "affected_by_change"),
        "rust work affected"
    );
    assert!(
        reasons(&plan)
            .iter()
            .filter(|(task, _)| task.starts_with("stack/tofu/"))
            .all(|(_, reason)| *reason != "affected_by_change"),
        "tofu triples unaffected by the rust touch"
    );
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-mixed",
        crates: 2,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "touch=rust-leaf",
    });
    Ok(())
}

/// Fork event: PR trust with the same selection as the PR plan.
#[test]
fn bench_tofu_fork_event() -> TestResult {
    let (repo, setup_ms) = timed(|| tofu_repo(BENCH_ROOTS));
    let repo = repo?;
    let root = repo.path();
    let (base, head) = commit_two_tofu(root, "stacks/r003/main.tf", TF_TOUCH)?;
    let (pr_plan, _) = plan_at_event(root, &base, &head, "pull_request")?;
    let (outcome, plan_ms, rss_kb) = timed_rss(|| plan_at_event(root, &base, &head, "fork"));
    let (plan, raw) = outcome?;
    assert_eq!(
        obligation_digest(&plan),
        obligation_digest(&pr_plan),
        "fork keeps the PR obligation set"
    );
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    assert_eq!(value["plan"]["trust"], "pr", "fork runs PR trust");
    let metadata_ms = index_baseline_ms(root)?;
    report(&BenchSample {
        case: "tofu-fork",
        crates: BENCH_ROOTS,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "event=fork",
    });
    Ok(())
}

/// A wide full plan fails closed on the matrix budget, never truncates.
#[test]
fn plan_150_roots_reports_matrix_budget() -> TestResult {
    let repo = tofu_repo(150)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let base = git_line(&["rev-parse", "HEAD"], root)?;
    for entry in fs::read_dir(root.join("stacks"))? {
        let main = entry?.path().join("main.tf");
        let mut body = fs::read_to_string(&main)?;
        body.push_str(TF_TOUCH);
        fs::write(&main, body)?;
    }
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let Err(err) = plan_at_event(root, &base, &head, "pull_request") else {
        return Err("150-root plan must fail closed".into());
    };
    assert!(
        err.to_string().contains("matrix_budget_exceeded"),
        "got {err}"
    );
    Ok(())
}
