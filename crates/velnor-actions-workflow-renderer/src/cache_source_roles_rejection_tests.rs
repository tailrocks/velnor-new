use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::StepKind;

#[test]
fn producer_mixed_computation_rejects_shell_internal_and_rust_helpers() {
    let mut shell = producer_job();
    shell.steps.push(
        crate::steps::ambient_shell_step(
            "Ordinary shell computation",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )
        .expect("shell step"),
    );
    assert_rejected(shell, "source_producer_mixed_computation");
    let mut internal = producer_job();
    internal.steps.push(crate::steps::plan_step());
    assert_rejected(internal, "source_producer_mixed_computation");
    let mut rust_helper = producer_job();
    rust_helper.steps.push(source_helper(
        "Prepare Rust",
        SourceBoundOperation::RustPrepareRootLinux,
        &["rust@1.98.1"],
        "rust-prepare",
    ));
    assert_rejected(rust_helper, "source_producer_mixed_computation");
}

#[test]
fn native_helper_without_metadata_is_rejected() {
    let mut job = producer_job();
    job.source_producer = None;
    assert_rejected(job, "source_producer_missing_role");
}

#[test]
fn checkout_and_adversarial_actions_are_rejected() {
    let mut checkout = producer_job();
    checkout.steps.insert(
        0,
        crate::checkout_step("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1")
            .expect("checkout"),
    );
    assert_rejected(checkout, "source_producer_bootstrap_prefix_changed");
    let mut adversarial = producer_job();
    adversarial.steps.insert(
        0,
        crate::action_step(
            "Adversarial action",
            "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            BTreeMap::new(),
        )
        .expect("adversarial action"),
    );
    assert_rejected(adversarial, "source_producer_bootstrap_prefix_changed");
}

#[test]
fn missing_terminal_report_is_rejected() {
    let mut job = producer_job();
    job.steps.pop();
    assert_rejected(job, "source_producer_missing_terminal_report");
}

#[test]
fn terminal_report_must_run_always() {
    let mut job = producer_job();
    job.steps.last_mut().expect("terminal report").condition = Some("success()".to_owned());
    assert_rejected(job, "source_producer_missing_terminal_report");
}

#[test]
fn duplicate_snapshot_is_rejected() {
    let mut job = producer_job();
    let duplicate = job
        .steps
        .iter()
        .find(|step| step.id.as_ref().is_some_and(|id| id.as_str() == BEFORE_ID))
        .cloned()
        .expect("before snapshot");
    job.steps.insert(6, duplicate);
    assert_rejected(job, "source_producer_snapshot_count_changed");
}

#[test]
fn partial_source_path_is_rejected() {
    let mut job = producer_job();
    let restore = job
        .steps
        .iter_mut()
        .find(|step| step.id.as_ref().is_some_and(|id| id.as_str() == RESTORE_ID))
        .expect("source restore");
    let StepKind::Action { with, .. } = &mut restore.kind else {
        panic!("source restore action")
    };
    with.insert(
        "path".to_owned(),
        NPM_PATHS.lines().next().expect("path").to_owned(),
    );
    assert_rejected(job, "source_producer_mixed_computation");
}

#[test]
fn publication_lookup_binding_is_rejected() {
    let mut job = producer_job();
    let publication = job
        .steps
        .iter_mut()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == PUBLICATION_ID)
        })
        .expect("publication lookup");
    let StepKind::Action { with, .. } = &mut publication.kind else {
        panic!("publication action")
    };
    with.insert("lookup-only".to_owned(), "false".to_owned());
    assert_rejected(job, "source_producer_mixed_computation");
}

#[test]
fn extra_action_is_rejected() {
    let mut job = producer_job();
    job.steps.insert(
        job.steps.len() - 1,
        crate::action_step(
            "Unexpected action",
            crate::steps::TOOLS_RESTORE_USES,
            BTreeMap::new(),
        )
        .expect("unexpected action"),
    );
    assert_rejected(job, "source_producer_mixed_computation");
}
