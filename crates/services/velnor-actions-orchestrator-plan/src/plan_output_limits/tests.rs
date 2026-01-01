use super::{
    JOB_OUTPUTS_BUDGET_UTF16_BYTES, MATRIX_JOB_LIMIT, PlanOutputMode, check_plan_outputs,
    output_record_utf16_bytes,
};

#[test]
fn utf16_budget_counts_ascii_and_surrogate_pairs() {
    assert_eq!(output_record_utf16_bytes("x", "A"), Some(8));
    assert_eq!(output_record_utf16_bytes("x", "🧪"), Some(10));
    assert_eq!(output_record_utf16_bytes("x", "A🧪"), Some(12));
}

#[test]
fn matrix_jobs_cap_uses_expanded_dynamic_entries_only() {
    assert!(
        check_plan_outputs(
            PlanOutputMode::DynamicMatrix,
            MATRIX_JOB_LIMIT,
            &[
                ("matrix", "{}"),
                ("plan_id", "plan-r1-a1"),
                ("run_key", "r1-a1")
            ],
        )
        .is_ok()
    );
    assert!(
        check_plan_outputs(
            PlanOutputMode::DynamicMatrix,
            MATRIX_JOB_LIMIT + 1,
            &[
                ("matrix", "{}"),
                ("plan_id", "plan-r1-a1"),
                ("run_key", "r1-a1")
            ],
        )
        .is_err_and(|error| error.to_string().contains("matrix_jobs_exceeded:257"))
    );
    assert!(
        check_plan_outputs(
            PlanOutputMode::Static,
            MATRIX_JOB_LIMIT + 1,
            &[("covered_tasks", "")],
        )
        .is_ok(),
        "static artifact rows do not expand into matrix jobs"
    );
}

#[test]
fn matrix_and_coverage_outputs_share_one_aggregate_budget() {
    let matrix = "m".repeat(400_000);
    let covered = "c".repeat(51_000);
    assert!(matrix.len() * 2 < JOB_OUTPUTS_BUDGET_UTF16_BYTES);
    assert!(covered.len() * 2 < JOB_OUTPUTS_BUDGET_UTF16_BYTES);
    assert!(
        check_plan_outputs(
            PlanOutputMode::DynamicMatrix,
            1,
            &[
                ("matrix", &matrix),
                ("plan_id", "plan-r1-a1"),
                ("run_key", "r1-a1"),
                ("covered_tasks", &covered),
                ("qualification_campaign", "qualification-campaign-token"),
            ],
        )
        .is_err_and(|error| error.to_string().contains("job_outputs_budget_exceeded"))
    );
}

#[test]
fn output_budget_boundary_is_inclusive() {
    let key_units = "covered_tasks".encode_utf16().count() + 2;
    let value_units = JOB_OUTPUTS_BUDGET_UTF16_BYTES / 2 - key_units;
    let exact = "x".repeat(value_units);
    assert!(
        check_plan_outputs(PlanOutputMode::Static, 0, &[("covered_tasks", &exact)]).is_ok(),
        "the configured output budget itself is accepted"
    );
    let over = format!("{exact}x");
    assert!(
        check_plan_outputs(PlanOutputMode::Static, 0, &[("covered_tasks", &over)])
            .is_err_and(|error| error.to_string().contains("job_outputs_budget_exceeded"))
    );
}
