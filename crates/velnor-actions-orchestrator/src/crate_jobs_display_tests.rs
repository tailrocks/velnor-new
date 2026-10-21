//! Crate-job display-partition tests (T20).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`: ID
//! and display prefixes derive from the same all-tofu partition, so
//! they can never disagree; collocated Rust and Tofu obligations keep separate owners.

use super::crate_jobs_tests::{discovery, group};
use super::*;
use velnor_actions_rust::TaskKind;

/// Tofu proposal via the adapter constructor.
fn tofu_group(root: &str, kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: root.to_owned(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn id_and_display_prefixes_agree_per_partition() {
    use velnor_actions_contract::{TOFU_DISPLAY_PREFIX, TOFU_JOB_ID_PREFIX};
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = group("demo", TaskKind::Clippy, &[]);
    let mixed_tofu = tofu_group("stacks/b", TofuTaskKind::Validate);
    rust.identity
        .unit_id
        .clone_from(&mixed_tofu.identity.unit_id);
    rust.configuration.clone_from(&mixed_tofu.configuration);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![
            group("demo", TaskKind::Clippy, &[]),
            tofu_group("stacks/a", TofuTaskKind::Validate),
            rust,
            mixed_tofu,
        ]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 4, "one job for each package or root");
    for (id, job) in &found.jobs {
        let tofu_id = id.starts_with(TOFU_JOB_ID_PREFIX);
        let tofu_display = job.display_name.starts_with(TOFU_DISPLAY_PREFIX);
        assert_eq!(
            tofu_id, tofu_display,
            "{id} pairs with {}",
            job.display_name
        );
    }
    let display = |id: &str| {
        found
            .jobs
            .iter()
            .find(|(job_id, _)| job_id == id)
            .map(|(_, job)| job.display_name.clone())
            .expect("built job")
    };
    assert_eq!(display("rust-demo"), "Rust / demo (Cargo.toml)");
    assert_eq!(display("tofu-stacks-a"), "OpenToFu — stacks/a");
    assert_eq!(display("tofu-stacks-b"), "OpenToFu — stacks/b");
    let rust_jobs: Vec<_> = found
        .jobs
        .iter()
        .filter(|(id, _)| id.starts_with("rust-"))
        .collect();
    assert_eq!(rust_jobs.len(), 2);
    assert!(rust_jobs.iter().all(|(id, _)| !id.ends_with("-lint")));
    assert!(
        rust_jobs
            .iter()
            .all(|(_, job)| job.display_name.starts_with("Rust / "))
    );
}
