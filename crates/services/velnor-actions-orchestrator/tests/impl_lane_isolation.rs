//! Lane isolation: no active concurrent writers share a target dir.
//!
//! Investigation (kept here so the negative result stays greppable):
//! no generated path runs two Cargo writers against one target dir, so
//! no distinct target-dir wiring exists and none is invented.
//!
//! - Matrix legs run on separate runners with fresh filesystems; the
//!   `strategy.max-parallel` cap limits leg count, never co-locates
//!   writers on one runner.
//! - Steps within one leg run sequentially: V1 emits no
//!   `parallel:`/`background:` step syntax.
//! - Test shards expand into separate matrix entries (separate legs),
//!   never co-located steps — and partitioning stays off by default
//!   (`test_sharding.default_shards = 1`).
//!
//! The lane helpers (`lane_cargo_target_env`, the `cargo_target_dir`
//! plan metadata) therefore have no YAML consumer for rust: the
//! checked-in `ci.yml` carries zero `CARGO_TARGET_DIR`. The rust test
//! below pins that disabled status: the partition gate stays at one
//! shard and the rendered workflow emits no lane target-dir wiring.
//! Wiring distinct target dirs becomes required only if a future path
//! co-locates concurrent Cargo writers on one runner.
//!
//! T18 flip: lanes went live for tofu validation roots. Root jobs fan
//! out one per root and stage through `needs` in lanes of
//! `max_parallel_jobs` (job `i` waits for job `i - max`), so same-lane
//! jobs serialize while lanes run free; each root job declares the
//! cap as `strategy.max-parallel`. The tofu test below pins the
//! two-lane shape; rust behavior above is unchanged.

use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::Job;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::{
    FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID, WORKFLOW_PATH,
};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// True for generated crate jobs (neither plan, lint, gate, nor publish).
fn is_crate_job(id: &str) -> bool {
    id != PLAN_JOB_ID && id != "actionlint" && id != FINAL_JOB_ID && id != PUBLISH_JOB_ID
}

#[test]
fn rust_partitions_stay_off_and_no_lane_wiring_renders() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    assert_eq!(
        prep.config.test_sharding.default_shards, 1,
        "partition gate stays off by default"
    );
    assert!(
        prep.config.test_sharding.by_manifest.is_empty(),
        "no manifest shard overrides by default"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert!(
        !yaml.contains("CARGO_TARGET_DIR"),
        "no lane target-dir wiring without concurrent writers"
    );
    Ok(())
}

#[test]
fn tofu_root_lanes_serialize_per_lane() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 2\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\", \"stacks/c\", \"stacks/d\"]\n";
    let repo = make_repo(config)?;
    for root in ["stacks/a", "stacks/b", "stacks/c", "stacks/d"] {
        fs::create_dir_all(repo.path().join(root))?;
        fs::write(
            repo.path().join(format!("{root}/main.tf")),
            "variable \"x\" {}\n",
        )?;
    }
    let prep = prepare(repo.path())?;
    let jobs: BTreeMap<String, Job> = finalized_jobs(&prep)?;
    let mut tofu: Vec<&String> = jobs
        .keys()
        .filter(|id| {
            is_crate_job(id)
                && jobs[*id].steps.iter().any(|step| {
                    step.name == "Validate"
                        && matches!(
                            &step.kind,
                            velnor_actions_contract::StepKind::Shell { env, .. }
                            if env
                                .get("VELNOR_TASK_ID")
                                .is_some_and(|task| task.starts_with("stack/tofu/"))
                        )
                })
        })
        .collect();
    tofu.sort();
    assert_eq!(tofu.len(), 4, "one job per root");
    assert_eq!(jobs[tofu[0]].needs, vec![PLAN_JOB_ID.to_owned()]);
    assert_eq!(jobs[tofu[1]].needs, vec![PLAN_JOB_ID.to_owned()]);
    assert_eq!(
        jobs[tofu[2]].needs,
        vec![PLAN_JOB_ID.to_owned(), tofu[0].clone()],
        "lane zero serializes"
    );
    assert_eq!(
        jobs[tofu[3]].needs,
        vec![PLAN_JOB_ID.to_owned(), tofu[1].clone()],
        "lane one serializes"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert_eq!(
        yaml.matches("max-parallel: 2").count(),
        4,
        "every root job declares the cap:\n{yaml}"
    );
    Ok(())
}
