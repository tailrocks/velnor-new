//! Final-gate job constructor tests.
//!
//! Declared from `workflow_jobs_tests.rs` under `cfg(test)`.

use super::*;

#[test]
fn final_job_writes_request_before_merge() {
    let catalog = ToolCatalog::pinned();
    for acquire in [None, Some(checkout_action().expect("checkout step"))] {
        let job = final_job("ubuntu-26.04", &["rust-demo".to_owned()], acquire, &catalog)
            .expect("final job");
        assert_request_before(
            &job,
            "Merge reports",
            "write-request-v1:merge-v1",
            MERGE_OPERATION,
        );
    }
}

#[test]
fn final_job_needs_plan_crates_and_lint() {
    let catalog = ToolCatalog::pinned();
    let job = final_job(
        "ubuntu-26.04",
        &["rust-demo".to_owned(), "rust-nested".to_owned()],
        None,
        &catalog,
    )
    .expect("final job");
    assert_eq!(
        job.needs,
        [
            PLAN_JOB_ID.to_owned(),
            "rust-demo".to_owned(),
            "rust-nested".to_owned(),
            LINT_JOB_ID.to_owned(),
        ]
    );
    let job = final_job("ubuntu-26.04", &[], None, &catalog).expect("final job");
    assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()]);
}
