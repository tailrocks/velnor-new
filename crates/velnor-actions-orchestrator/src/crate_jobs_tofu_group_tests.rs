use super::*;

#[test]
fn all_tofu_groups_take_tofu_ids_and_rust_keeps_its_id() {
    use velnor_actions_contract::TOFU_JOB_ID_PREFIX;
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![
            rust,
            tofu,
            tofu_group("stacks/b", TofuTaskKind::Validate),
        ]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 3);
    let mut ids: Vec<&str> = found.jobs.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 3);
    assert!(
        ids[0].starts_with("rust-"),
        "mixed group keeps the rust union: {ids:?}"
    );
    assert_eq!(ids[1], format!("{TOFU_JOB_ID_PREFIX}stacks-a"));
    assert_eq!(ids[2], format!("{TOFU_JOB_ID_PREFIX}stacks-b"));
    for (id, job) in &found.jobs {
        assert!(
            velnor_actions_contract::is_crate_job_id(id),
            "{id} stays a crate job"
        );
        assert_eq!(job.needs, vec![PLAN_JOB_ID.to_owned()], "{id} needs plan");
    }
}
