//! Configured `workflow.crate_timeout_minutes` reaches every crate job.
//!
//! Declared via `#[path]` from `crate_jobs_tests.rs` under `cfg(test)`.

use super::*;

#[test]
fn configured_timeout_reaches_one_job_for_each_rust_package() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let doc = group("demo", TaskKind::Doc, &[clippy_id.as_str()]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![clippy, test, doc]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::new(30).expect("valid timeout"),
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    for (id, job) in &found.jobs {
        assert_eq!(job.timeout_minutes.minutes(), 30, "{id}");
    }
}

#[test]
fn configured_timeout_reaches_every_tofu_root_job() {
    let mut tasks = Vec::new();
    for root in ["stacks/a", "stacks/b"] {
        for kind in [
            velnor_actions_tofu::TofuTaskKind::Fmt,
            velnor_actions_tofu::TofuTaskKind::InitForValidate,
            velnor_actions_tofu::TofuTaskKind::Validate,
        ] {
            let group = velnor_actions_tofu::TofuTaskGroup {
                root: root.to_owned(),
                kind,
                configuration: "default".to_owned(),
                no_targets: false,
            };
            tasks.push(velnor_actions_tofu::propose_task(&group).expect("fixture proposes"));
        }
    }
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::new(120).expect("valid timeout"),
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    for (id, job) in &found.jobs {
        assert_eq!(job.timeout_minutes.minutes(), 120, "{id}");
    }
}
