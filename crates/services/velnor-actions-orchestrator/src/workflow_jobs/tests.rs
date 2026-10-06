//! Workflow job constructor tests.
//!
//! Declared via `#[path]` from `workflow_jobs.rs` under `cfg(test)`.

use super::*;
use velnor_actions_contract::StepKind;

/// Internal operation of one step, if any.
fn operation_of(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Internal { operation, .. } => Some(operation),
        StepKind::Action { .. } | StepKind::Shell { .. } => None,
    }
}

/// Assert Write request precedes `target` with the expected operations.
fn assert_request_before(job: &Job, target: &str, request: &str, operation: &str) {
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    let write_at = names.iter().position(|name| *name == "Write request");
    let target_at = names.iter().position(|name| *name == target);
    assert!(
        write_at.is_some_and(|write| Some(write) < target_at),
        "request must precede {target}: {names:?}"
    );
    assert_eq!(
        operation_of(&job.steps[write_at.expect("write request step")]),
        Some(request),
        "request must target {target}"
    );
    assert_eq!(
        operation_of(&job.steps[target_at.expect("target step")]),
        Some(operation)
    );
}

mod workflow_jobs_tests;
