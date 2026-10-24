use super::super::crate_jobs_tests::group;
use super::external_needs;
use crate::crate_job_ids::JobOwners;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_rust::TaskKind;

#[test]
fn cross_package_gate_names_the_external_owner() {
    let first = group("core", TaskKind::Clippy, &[]);
    let mut second = group("app", TaskKind::Test, &[first.task_id.as_str()]);
    second.depends_on.push(first.task_id.clone());
    let proposals = vec![first.clone(), second.clone()];

    let owners = JobOwners::new(&proposals).expect("owners");
    let needs = external_needs(&[&second], &owners, "rust-app", WorkflowEvent::PullRequest)
        .expect("valid cross-package gate");

    assert_eq!(needs, ["plan", "rust-core"]);
}

#[test]
fn same_package_gate_stays_inside_one_job() {
    let first = group("core", TaskKind::Clippy, &[]);
    let second = group("core", TaskKind::Test, &[first.task_id.as_str()]);
    let proposals = vec![first, second.clone()];

    let owners = JobOwners::new(&proposals).expect("owners");
    let needs = external_needs(&[&second], &owners, "rust-core", WorkflowEvent::PullRequest)
        .expect("valid same-package gate");

    assert_eq!(needs, ["plan"]);
}

#[test]
fn repeated_external_gates_add_one_owner_dependency() {
    let first = group("core", TaskKind::Clippy, &[]);
    let second = group("core", TaskKind::Test, &[]);
    let mut app = group("app", TaskKind::Test, &[first.task_id.as_str()]);
    app.gated_by.push(second.task_id.clone());
    let proposals = vec![first, second, app.clone()];

    let owners = JobOwners::new(&proposals).expect("owners");
    let needs = external_needs(&[&app], &owners, "rust-app", WorkflowEvent::PullRequest)
        .expect("valid repeated gates");

    assert_eq!(needs, ["plan", "rust-core"]);
}

#[test]
fn no_target_gate_does_not_add_a_job_dependency() {
    let mut doctest = group("demo", TaskKind::Doctest, &[]);
    doctest.no_targets = true;
    let docs = group("demo", TaskKind::Doc, &[doctest.task_id.as_str()]);
    let proposals = vec![doctest, docs.clone()];

    let owners = JobOwners::new(&proposals).expect("owners");
    let needs = external_needs(&[&docs], &owners, "rust-demo", WorkflowEvent::PullRequest)
        .expect("valid no-target gate");

    assert_eq!(needs, ["plan"]);
}
