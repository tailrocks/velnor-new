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

#[test]
fn static_verification_producer_does_not_require_or_emit_matrix_outputs() {
    use velnor_actions_contract_config::{
        ArtifactBuildOutput, ArtifactBuildTask, VerificationRunner,
    };
    use velnor_actions_contract_workflow::{
        ArtifactBuildProducer, ArtifactBuildProvider, ArtifactBuildTaskPlan, Plan, PlanBaseline,
        PlanGenerator, PlanMatrix, PlanRunner, Trust, WorkflowEvent,
    };

    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: "r1-a1".to_owned(),
        plan_id: "plan-r1-a1".to_owned(),
        base: None,
        head: "a".repeat(40),
        event: WorkflowEvent::Push,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: velnor_actions_contract_config::RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        baseline: PlanBaseline::unavailable(Some("no_baseline")).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.1".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: Vec::new(),
        artifact_tasks: vec![ArtifactBuildTaskPlan {
            task: ArtifactBuildTask {
                id: "frontend-check".to_owned(),
                mise_task: "lint-frontend".to_owned(),
                runner: VerificationRunner::LinuxX64,
                timeout_minutes: 10,
                outputs: vec![ArtifactBuildOutput {
                    id: "lint-report".to_owned(),
                    path: "dist/lint-report.json".to_owned(),
                    max_bytes: 4096,
                }],
            },
            producer: ArtifactBuildProducer::VerificationTask,
            providers: vec![ArtifactBuildProvider::GithubHosted],
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    let response = crate::internal::PlanResponse {
        schema: 1,
        matrix: plan.matrix.clone(),
        plan,
        baseline_manifest: None,
    };
    let response = serde_json::to_string(&response).expect("serialize plan response");
    let outputs = plan_outputs(&response, PlanOutputMode::Static)
        .expect("static verification outputs do not need matrix promotion");
    assert!(outputs.artifact_hosted_matrix.is_none());
    assert!(outputs.artifact_velnor_matrix.is_none());
    assert_eq!(
        outputs
            .step_outputs()
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["matrix", "plan_id", "run_key", "covered_tasks"]
    );
}
