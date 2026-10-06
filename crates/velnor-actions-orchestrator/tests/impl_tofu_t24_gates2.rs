//! T24 efficiency gates 5–7 (spec §9) as named invariant tests.
//!
//! Size split from `impl_tofu_t24_gates` (gates 1–4); shares its
//! scan/job helpers plus the fixture helper through `crate::`.

use velnor_actions_contract::{JobTimeout, digest_b3};
use velnor_actions_mise::command::{OUTPUT_CAPTURE_LIMIT_BYTES, RUN_TIMEOUT_SECS};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_tofu::kinds::TofuTaskKind;
use velnor_actions_tofu::task_identity::{DigestSlot, ExtensionInputs, TofuTaskIdentityExtension};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID, WORKFLOW_PATH};

use crate::impl_common::TestResult;
use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::{
    commit_two_tofu, plan_at_event, tofu_repo,
};
use crate::impl_tofu_t24_gates::{crate_src, is_crate_job, token_hits};

/// Extension inputs over fixed digests for one root/kind.
fn gate_inputs<'a>(
    lock: DigestSlot,
    kind: TofuTaskKind,
    workspace: &'a str,
    graph: &'a str,
    config: &'a str,
) -> ExtensionInputs<'a> {
    ExtensionInputs {
        unit_id: "dir-737461636b732f61",
        workspace_id: workspace,
        profile: "default",
        manifest: "stacks/a",
        graph_digest: graph,
        root: "stacks/a",
        config_digest: config,
        lock_digest: lock,
        kind,
        undeclared_reads: false,
        declared_inputs: &[],
    }
}

/// Gate 5: "Cache misses retain all correctness gates. A cache hit
/// does not replace `validate` execution under the initial policy."
/// (T23 verdicts: init/validate reuse OFF, fmt qualified, envelope valid.)
#[test]
fn gate5_cache_hit_never_replaces_validate() {
    let workspace = digest_b3(b"workspace");
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let lock = || DigestSlot::Known(digest_b3(b"lock"));
    for kind in [TofuTaskKind::InitForValidate, TofuTaskKind::Validate] {
        let ext = TofuTaskIdentityExtension::for_task(&gate_inputs(
            lock(),
            kind,
            &workspace,
            &graph,
            &config,
        ));
        let err = ext
            .reuse_eligible()
            .expect_err("init/validate reuse stays OFF");
        assert!(err.to_string().contains("tofu_reuse_disabled"), "got {err}");
        assert!(
            ext.coverage_eligible().is_ok(),
            "coverage untouched by the reuse refusal"
        );
    }
    let fmt = TofuTaskIdentityExtension::for_task(&gate_inputs(
        lock(),
        TofuTaskKind::Fmt,
        &workspace,
        &graph,
        &config,
    ));
    assert!(
        fmt.reuse_eligible().is_ok(),
        "fmt keeps the shared qualification"
    );
}

/// Gate 6: "Existing resource limits bound root fan-out and
/// subprocess budgets. Do not split trivial steps into separate
/// hosted jobs without measured benefit."
#[test]
fn gate6_limits_bound_fanout_and_subprocess_budgets() -> TestResult {
    assert_eq!(RUN_TIMEOUT_SECS, 600, "run deadline");
    assert_eq!(
        OUTPUT_CAPTURE_LIMIT_BYTES,
        8 * 1024 * 1024,
        "capture fails closed past 8 MiB"
    );
    assert_eq!(JobTimeout::PLAN.minutes(), 20);
    assert_eq!(JobTimeout::CRATE.minutes(), 30);
    assert_eq!(JobTimeout::REQUIRED.minutes(), 10);
    assert_eq!(JobTimeout::VALIDATOR.minutes(), 10);
    let renderer = crate_src("../velnor-actions-workflow-renderer");
    assert!(
        token_hits(&renderer, "MATRIX_MAX_PARALLEL_ENV")?
            .iter()
            .any(|hit| hit.starts_with("matrix.rs:")),
        "matrix cap env stays wired"
    );
    let dir = tofu_repo(3)?;
    let first = render_staged_tree(&prepare(dir.path())?)?;
    let second = render_staged_tree(&prepare(dir.path())?)?;
    let yaml = first.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let again = second.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert_eq!(yaml, again, "staging deterministic for a fixed ID set");
    assert!(
        yaml.contains("max-parallel: 2"),
        "root fan-out honors the cap"
    );
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let ids: Vec<&String> = jobs.keys().filter(|id| is_crate_job(id)).collect();
    assert_eq!(ids.len(), 3, "one job per root, never per step");
    for (id, job) in &jobs {
        let minutes = job.timeout_minutes.minutes();
        assert!(
            (JobTimeout::MIN_MINUTES..=JobTimeout::MAX_MINUTES).contains(&minutes),
            "{id} bounded: {minutes}"
        );
    }
    assert_eq!(
        jobs.get(PLAN_JOB_ID).ok_or("plan job")?.timeout_minutes,
        JobTimeout::PLAN
    );
    for id in &ids {
        assert_eq!(
            jobs.get(*id).ok_or("crate job")?.timeout_minutes,
            JobTimeout::CRATE,
            "{id} carries the crate bound"
        );
    }
    Ok(())
}

/// Gate 7 (derived from the §9 budget table docs-only row): "No tofu
/// installation/initialization/validation when the plan has proved no
/// relevant tofu obligation needs execution. Generic mandatory
/// workflow checks remain."
#[test]
fn gate7_docs_only_runs_zero_tofu_operations() -> TestResult {
    let dir = tofu_repo(2)?;
    let root = dir.path();
    let (base, head) = commit_two_tofu(root, "README.md", "more docs\n")?;
    let (plan, _) = plan_at_event(root, &base, &head, "pull_request")?;
    assert!(!plan.obligations.is_empty(), "universe still planned");
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.reason != "affected_by_change"),
        "no affected tofu work: {:?}",
        plan.obligations
            .iter()
            .map(|ob| (&ob.task_id, &ob.reason))
            .collect::<Vec<_>>()
    );
    for name in ["stacks/r000", "stacks/r001"] {
        let key = velnor_actions_tofu::key_for_root(name);
        assert!(
            plan.packages.iter().any(|row| row.package_id == key),
            "{name} row present"
        );
    }
    let jobs = finalized_jobs(&prepare(root)?)?;
    assert!(jobs.contains_key(PLAN_JOB_ID), "plan check remains");
    assert!(jobs.contains_key(FINAL_JOB_ID), "required check remains");
    assert!(
        render_staged_tree(&prepare(root)?)?
            .get(WORKFLOW_PATH)
            .is_some(),
        "workflow still renders"
    );
    Ok(())
}
