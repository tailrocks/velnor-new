use super::super::crate_jobs_tests::group;
use super::external_needs;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_rust::TaskKind;

#[test]
fn cross_package_gate_names_the_external_owner() {
    let first = group("core", TaskKind::Clippy, &[]);
    let mut second = group("app", TaskKind::Test, &[first.task_id.as_str()]);
    second.depends_on.push(first.task_id.clone());
    let proposals = vec![first.clone(), second.clone()];

    let needs = external_needs(
        &[&second],
        &proposals,
        "rust-app",
        WorkflowEvent::PullRequest,
    )
    .expect("valid cross-package gate");

    assert_eq!(needs, ["plan", "rust-core"]);
}

#[test]
fn same_package_gate_stays_inside_one_job() {
    let first = group("core", TaskKind::Clippy, &[]);
    let second = group("core", TaskKind::Test, &[first.task_id.as_str()]);
    let proposals = vec![first, second.clone()];

    let needs = external_needs(
        &[&second],
        &proposals,
        "rust-core",
        WorkflowEvent::PullRequest,
    )
    .expect("valid same-package gate");

    assert_eq!(needs, ["plan"]);
}
