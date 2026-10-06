use super::{
    JOB_OUTPUTS_BUDGET_UTF16_BYTES, MATRIX_JOB_LIMIT, PlanOutputMode, check_plan_outputs,
    output_record_utf16_bytes,
};
use crate::PlanOutputs;

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
fn step_outputs_remain_complete_while_static_job_promotion_is_narrow() {
    let outputs = PlanOutputs {
        matrix: "{\"include\":[]}".to_owned(),
        plan_id: "plan-r1-a1".to_owned(),
        run_key: "r1-a1".to_owned(),
        covered_tasks: String::new(),
        qualification_campaign: "release-1".to_owned(),
        qualification_phase: "cold".to_owned(),
        qualification_cache_enabled: true,
        qualification_cache_write: true,
        qualification_cache_directives: "{\"schema\":1}".to_owned(),
        job_outputs_utf16_bytes: 0,
    };
    let step_names: Vec<&str> = outputs
        .step_outputs()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        step_names,
        [
            "matrix",
            "plan_id",
            "run_key",
            "covered_tasks",
            "qualification_campaign",
            "qualification_phase",
            "qualification_cache_enabled",
            "qualification_cache_write",
            "qualification_cache_directives",
        ]
    );
    assert_eq!(outputs.step_outputs()[6].1, "true");
    assert_eq!(outputs.step_outputs()[7].1, "true");
    let static_job_names: Vec<&str> = outputs
        .promoted_job_outputs(PlanOutputMode::Static)
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        static_job_names,
        [
            "covered_tasks",
            "qualification_campaign",
            "qualification_phase",
            "qualification_cache_enabled",
            "qualification_cache_write",
            "qualification_cache_directives",
        ]
    );
    let dynamic_job_names: Vec<&str> = outputs
        .promoted_job_outputs(PlanOutputMode::DynamicMatrix)
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(dynamic_job_names, step_names);
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
                ("qualification_phase", "useful_delta"),
                ("qualification_cache_enabled", "true"),
                ("qualification_cache_write", "true"),
                ("qualification_cache_directives", "{\"schema\":1}"),
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
