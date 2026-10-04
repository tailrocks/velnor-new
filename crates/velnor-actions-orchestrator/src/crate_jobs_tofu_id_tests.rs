//! Tofu job identifiers preserve stack identity.

use super::*;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

#[test]
fn all_tofu_groups_take_tofu_ids_mixed_keep_rust() {
    use velnor_actions_contract::TOFU_JOB_ID_PREFIX;
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![
            rust,
            tofu,
            tofu_group("stacks/b", TofuTaskKind::Validate),
        ]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    let mut ids: Vec<&str> = found.jobs.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 2);
    assert!(
        ids[0].starts_with("rust-"),
        "mixed group keeps the rust union: {ids:?}"
    );
    assert_eq!(ids[1], format!("{TOFU_JOB_ID_PREFIX}stacks-b"));
    for (id, job) in &found.jobs {
        assert!(
            velnor_actions_contract::is_crate_job_id(id),
            "{id} stays a crate job"
        );
        assert_eq!(job.needs, vec![PLAN_JOB_ID.to_owned()], "{id} needs plan");
    }
}
