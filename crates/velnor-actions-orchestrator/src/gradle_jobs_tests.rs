//! Helper tests for Gradle job construction.

use super::*;

#[test]
fn job_ids_are_slugged_and_scoped_by_root() {
    assert_eq!(
        job_id("backend", ":eth-domain"),
        "gradle-backend-eth-domain"
    );
    assert_eq!(job_id("", ":a:b"), "gradle-a-b");
}

#[test]
fn the_run_step_is_the_fixed_internal_operation_with_closed_env() {
    let step = run_step("backend", ":core", 4, true);
    let StepKind::Internal { operation, env } = &step.kind else {
        panic!("internal step");
    };
    assert_eq!(operation, GRADLE_RUN_OPERATION);
    assert_eq!(env[TASKS_ENV], ":core:assemble :core:check");
    assert_eq!(env[DATABASE_ENV], "1");
    assert_eq!(env[JAVA_ENV], "${VELNOR_TOOL_JAVA}");
    assert_eq!(env.len(), 5);
}
