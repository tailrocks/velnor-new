//! Plan-output promotion: complete step outputs, narrow static jobs.

use super::*;

#[test]
fn step_outputs_remain_complete_while_static_job_promotion_is_narrow() {
    let outputs = PlanOutputs {
        matrix: "{\"include\":[]}".to_owned(),
        plan_id: "plan-r1-a1".to_owned(),
        run_key: "r1-a1".to_owned(),
        covered_tasks: String::new(),
        artifact_hosted_matrix: None,
        artifact_velnor_matrix: None,
        job_outputs_utf16_bytes: 0,
    };
    let step_names: Vec<&str> = outputs
        .step_outputs()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        step_names,
        ["matrix", "plan_id", "run_key", "covered_tasks"]
    );
    let static_job_names: Vec<&str> = outputs
        .promoted_job_outputs(PlanOutputMode::Static)
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(static_job_names, ["covered_tasks"]);
    let dynamic_job_names: Vec<&str> = outputs
        .promoted_job_outputs(PlanOutputMode::DynamicMatrix)
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(dynamic_job_names, step_names);
}

#[test]
fn selected_artifact_provider_matrices_are_promoted_to_plan_job_outputs() {
    let outputs = PlanOutputs {
        matrix: "{\"include\":[]}".to_owned(),
        plan_id: "plan-r1-a1".to_owned(),
        run_key: "r1-a1".to_owned(),
        covered_tasks: String::new(),
        artifact_hosted_matrix: Some("{\"include\":[{\"provider\":\"github_hosted\"}]}".to_owned()),
        artifact_velnor_matrix: Some(
            "{\"include\":[{\"provider\":\"velnor_scale_set\"}]}".to_owned(),
        ),
        job_outputs_utf16_bytes: 0,
    };
    let names: Vec<&str> = outputs
        .promoted_job_outputs(PlanOutputMode::DynamicMatrix)
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        names,
        [
            "matrix",
            "plan_id",
            "run_key",
            "covered_tasks",
            "artifact_hosted_matrix",
            "artifact_velnor_matrix",
        ]
    );
}
