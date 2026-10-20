//! The crate-job source probe follows the obligations' Cargo feature mode.

use super::*;

/// Source-probe shell line of the single crate job built from `tasks`.
fn crate_probe(tasks: Vec<ProposedTask>) -> String {
    let source_roots = [String::new()];
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &source_roots,
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    let fetch = found.jobs[0]
        .1
        .steps
        .iter()
        .find(|step| step.name == crate::source_prep::FETCH_SOURCES_STEP)
        .expect("crate source probe");
    let velnor_actions_contract::StepKind::Shell { run, .. } = &fetch.kind else {
        panic!("source probe must be a shell step");
    };
    run[2].clone()
}

#[test]
fn all_features_obligation_selects_all_features_source_probe() {
    let probe = crate_probe(vec![group_with_all_features(
        "demo",
        TaskKind::Clippy,
        &[],
        true,
    )]);
    assert!(
        probe.contains("metadata --locked --offline --all-features"),
        "{probe}"
    );
}

#[test]
fn default_feature_obligations_keep_the_existing_source_probe() {
    let probe = crate_probe(vec![group("demo", TaskKind::Clippy, &[])]);
    assert!(probe.contains("metadata --locked --offline "), "{probe}");
    assert!(!probe.contains("--all-features"), "{probe}");
}
