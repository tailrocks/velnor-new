//! Baseline-publish job tests: gate, needs, and step order.
//!
//! Declared via `#[path]` from `publish_job.rs` under `cfg(test)`.

use super::*;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};

/// Internal operation of one step, if any.
fn operation_of(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Internal { operation } => Some(operation),
        StepKind::Action { .. } | StepKind::Shell { .. } | StepKind::SourceBoundHelper { .. } => {
            None
        }
    }
}

#[test]
fn publish_job_needs_required_and_gates_push() {
    let job = baseline_publish_job("ubuntu-26.04", "testmain", None, &ToolCatalog::pinned())
        .expect("publish job");
    assert_eq!(job.display_name, PUBLISH_DISPLAY_NAME);
    assert_eq!(job.needs, [FINAL_JOB_ID.to_owned()]);
    assert_eq!(
        job.condition.as_deref(),
        Some(
            "github.event_name == 'push' && github.ref == 'refs/heads/testmain' && github.ref_protected == true"
        )
    );
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Download plan",
            "Download final report",
            "Prepare pinned tools",
            "Write request",
            "Publish baseline",
            "Upload baseline",
        ]
    );
    assert_eq!(
        operation_of(&job.steps[3]),
        Some("write-request-v1:publish-baseline-v1")
    );
    assert_eq!(operation_of(&job.steps[4]), Some(PUBLISH_OPERATION));
    assert_eq!(
        job.steps[5].condition.as_deref(),
        Some("success() && steps.publish-baseline.outputs.upload_needed == 'true'")
    );
    let StepKind::Action { uses, with, .. } = &job.steps[5].kind else {
        panic!("upload must be an action step");
    };
    assert_eq!(uses, UPLOAD_ARTIFACT_USES);
    assert_eq!(
        with.get("name").map(String::as_str),
        Some("${{ steps.publish-baseline.outputs.artifact_name }}")
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(
            "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ github.run_attempt }}/published/baseline.json"
        ),
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
    for bad in ["", "  ", "feat/x y", "a\nb"] {
        assert!(
            baseline_publish_job("ubuntu-26.04", bad, None, &ToolCatalog::pinned()).is_err(),
            "malformed branches never reach the gate: {bad:?}"
        );
    }
}

#[test]
fn publish_job_downloads_exact_current_final_report() {
    let job = baseline_publish_job("ubuntu-26.04", "main", None, &ToolCatalog::pinned())
        .expect("publish job");
    let StepKind::Action { uses, with, .. } = &job.steps[1].kind else {
        panic!("final download must be an action step");
    };
    assert_eq!(uses, DOWNLOAD_ARTIFACT_USES);
    assert_eq!(
        with.get("name").map(String::as_str),
        Some("velnor-final-r${{ github.run_id }}-a${{ github.run_attempt }}")
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(velnor_actions_workflow_renderer::closure::PLAN_ARTIFACT_PATH)
    );
    assert!(!with.contains_key("pattern"));
    assert!(!with.contains_key("run-id"));
    let StepKind::Shell { run, .. } = &job.steps[2].kind else {
        panic!("pinned gh preparation must be a shell step");
    };
    assert!(run.iter().any(|arg| arg.starts_with("gh@")));
    assert!(!run.iter().any(|arg| arg.starts_with("rust@")));
}
