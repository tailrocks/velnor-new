//! Baseline-publish job tests: gate, needs, and step order.
//!
//! Declared via `#[path]` from `publish_job.rs` under `cfg(test)`.

use super::*;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};

/// Internal operation of one step, if any.
fn operation_of(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Internal { operation, .. } => Some(operation),
        StepKind::Action { .. } | StepKind::Shell { .. } | StepKind::TaskExecution { .. } => None,
    }
}

#[test]
fn publish_job_needs_required_and_gates_push() {
    let job = baseline_publish_job("ubuntu-26.04", "testmain", None).expect("publish job");
    assert_eq!(job.display_name, PUBLISH_DISPLAY_NAME);
    assert_eq!(job.needs, [FINAL_JOB_ID.to_owned()]);
    assert_eq!(
        job.condition.as_deref(),
        Some("github.event_name == 'push' && github.ref == 'refs/heads/testmain'")
    );
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Download plan",
            "Write request",
            "Publish baseline",
            "Upload baseline",
        ]
    );
    assert_eq!(
        operation_of(&job.steps[1]),
        Some("write-request-v1:publish-baseline-v1")
    );
    assert_eq!(operation_of(&job.steps[2]), Some(PUBLISH_OPERATION));
    let StepKind::Action { uses, with, .. } = &job.steps[3].kind else {
        panic!("upload must be an action step");
    };
    assert_eq!(uses, UPLOAD_ARTIFACT_USES);
    assert_eq!(
        with.get("name").map(String::as_str),
        Some("${{ steps.publish-baseline.outputs.artifact_name }}")
    );
    assert!(
        with.get("path")
            .is_some_and(|path| path.ends_with("/published-baseline/baseline.json")),
        "upload stages output separately from its parent: {with:?}"
    );
    assert_eq!(
        with.get("if-no-files-found").map(String::as_str),
        Some("error")
    );
    let StepKind::Action { uses, .. } = &job.steps[0].kind else {
        panic!("plan download must be an action step");
    };
    assert_eq!(uses, DOWNLOAD_ARTIFACT_USES);
}

#[test]
fn publish_job_rejects_malformed_branches() {
    for bad in ["", "  ", "feat/x y", "a\nb", "main'||true||'"] {
        assert!(
            baseline_publish_job("ubuntu-26.04", bad, None).is_err(),
            "malformed branches never reach the gate: {bad:?}"
        );
    }
}
