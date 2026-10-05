//! Crate-job display-partition tests (T20).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`: ID
//! and display prefixes derive from the same all-tofu partition, so
//! they can never disagree; mixed groups keep the rust union.

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
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![
            group("demo", TaskKind::Clippy, &[]),
            tofu_group("stacks/a", TofuTaskKind::Validate),
            rust,
            mixed_tofu,
        ]),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 3, "rust plus tofu plus mixed");
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
    assert_eq!(display("rust-demo"), "Rust / demo");
    assert_eq!(display("tofu-stacks-a"), "OpenToFu — stacks/a");
    let mixed = found
        .jobs
        .iter()
        .find(|(id, _)| id.starts_with("rust-") && *id != "rust-demo")
        .expect("mixed group");
    assert!(
        mixed.1.display_name.starts_with("Rust / "),
        "mixed keeps rust: {}",
        mixed.1.display_name
    );
}
