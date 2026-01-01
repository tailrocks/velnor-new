//! Fail-closed matrix and job-output budgets.

use velnor_actions_orchestrator_plan::plan_output_limits::{
    JOB_OUTPUTS_BUDGET_UTF16_BYTES, PlanOutputMode, check_plan_outputs,
};

#[test]
fn static_mode_ignores_matrix_cardinality() {
    assert!(check_plan_outputs(PlanOutputMode::Static, 10_000, &[("covered_tasks", "")]).is_ok());
}

#[test]
fn dynamic_mode_caps_matrix_entries() {
    assert!(check_plan_outputs(PlanOutputMode::DynamicMatrix, 256, &[("matrix", "{}")]).is_ok());
    assert!(
        check_plan_outputs(PlanOutputMode::DynamicMatrix, 257, &[("matrix", "{}")])
            .is_err_and(|error| error.to_string().contains("matrix_jobs_exceeded:257"))
    );
}

#[test]
fn budget_counts_utf16_records() {
    assert_eq!(
        check_plan_outputs(PlanOutputMode::Static, 0, &[]).expect("empty"),
        0
    );
    assert_eq!(
        check_plan_outputs(PlanOutputMode::Static, 0, &[("x", "A")]).expect("record"),
        8
    );
    assert_eq!(JOB_OUTPUTS_BUDGET_UTF16_BYTES, 900_000);
}
