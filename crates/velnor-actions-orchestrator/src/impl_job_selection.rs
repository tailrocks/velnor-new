//! Runner allocation and staged-lane selection regression tests.

use super::stage_tofu_root_jobs;
use crate::crate_jobs::build_crate_jobs;
use crate::crate_jobs::crate_jobs_tests::{discovery, group};
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::TaskKind;

#[test]
fn generated_runner_is_guarded_before_any_setup() {
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![group("demo", TaskKind::Clippy, &[])]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
    .expect("jobs");
    let job = &found.jobs[0].1;
    assert_eq!(
        job.condition.as_deref(),
        Some(
            "!cancelled() && needs.plan.result == 'success' && (!contains(needs.plan.outputs.covered_tasks, ',stack/rust/root/clippy/default,'))"
        )
    );
    assert!(
        !job.steps.is_empty(),
        "selection is on the job, not its setup steps"
    );
}

#[test]
fn skipped_staged_predecessor_preserves_selected_successor_predicate() {
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![
            group("a", TaskKind::Clippy, &[]),
            group("b", TaskKind::Clippy, &[]),
        ]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
    .expect("jobs");
    let mut jobs = found.jobs;
    let ids = jobs.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let expected = jobs[1].1.condition.clone();
    stage_tofu_root_jobs(&mut jobs, &ids, 1);
    assert_eq!(jobs[1].1.condition, expected);
    assert!(jobs[1].1.needs.contains(&ids[0]));
    assert!(expected.expect("selection").contains("!cancelled()"));
}
