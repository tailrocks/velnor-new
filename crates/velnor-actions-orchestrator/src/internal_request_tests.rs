use super::{publish_plan_files, publish_plan_files_with_outputs};
use crate::OrchestratorError;
use crate::plan_output_limits::{PlanOutputMode, plan_outputs_with_admission};
use velnor_actions_contract::workflow::{Plan, PlanGenerator, PlanMatrix, PlanRunner};
use velnor_actions_contract::{
    QualificationDispatch, QualificationPhase, RunnerSelection, Trust, WorkflowEvent,
    plan_id_for_run, run_key_for_ci,
};

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn publishes_qualification_directives_beside_the_validated_plan() {
    let response = cold_qualification_response();
    let outputs = plan_outputs_with_admission(&response, PlanOutputMode::Static, None)
        .expect("cold plan outputs");
    assert!(outputs.qualification_cache_directives.starts_with('{'));

    let artifacts = tempfile::tempdir().expect("artifact root");
    let response_value: serde_json::Value = serde_json::from_str(&response).expect("response");
    let context: QualificationDispatch =
        serde_json::from_value(response_value["plan"]["qualification"].clone())
            .expect("qualification context");
    let snapshot = crate::qualification_cache_backend::complete_snapshot_for_test(&context)
        .expect("test snapshot");
    crate::qualification_cache_backend::stage_qualification_cache_before(
        artifacts.path(),
        &snapshot,
    )
    .expect("stage backend snapshot");
    let run_dir =
        publish_plan_files_with_outputs(&response, &artifacts.path().join("velnor"), &outputs)
            .expect("publish plan and directives");

    let directive_path =
        run_dir.join(crate::qualification_cache_runtime::QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
    assert_eq!(
        std::fs::read_to_string(directive_path).expect("staged directive"),
        outputs.qualification_cache_directives
    );
    assert!(
        run_dir
            .join(velnor_actions_contract::PLAN_JSON_FILENAME)
            .is_file()
    );
    assert!(
        run_dir
            .join(velnor_actions_contract::MATRIX_JSON_FILENAME)
            .is_file()
    );
}

#[test]
fn qualification_plan_without_directive_outputs_fails_before_artifacts() {
    let response = cold_qualification_response();
    let artifacts = tempfile::tempdir().expect("artifact root");

    let error = publish_plan_files(&response, &artifacts.path().join("velnor"))
        .expect_err("qualification artifact requires its typed directive");

    assert!(matches!(error, OrchestratorError::Internal { .. }));
    assert!(!artifacts.path().join("velnor").exists());
}

#[test]
fn qualification_plan_publication_requires_the_exact_before_snapshot() {
    let response = cold_qualification_response();
    let outputs = plan_outputs_with_admission(&response, PlanOutputMode::Static, None)
        .expect("cold plan outputs");
    let artifacts = tempfile::tempdir().expect("artifact root");
    let error =
        publish_plan_files_with_outputs(&response, &artifacts.path().join("velnor"), &outputs)
            .expect_err("missing before snapshot fails closed");
    assert!(matches!(error, OrchestratorError::Internal { .. }));
    assert!(!artifacts.path().join("velnor").exists());
}

#[test]
fn qualification_plan_publication_rejects_snapshot_from_another_dispatch() {
    let response = cold_qualification_response();
    let outputs = plan_outputs_with_admission(&response, PlanOutputMode::Static, None)
        .expect("cold plan outputs");
    let artifacts = tempfile::tempdir().expect("artifact root");
    let response_value: serde_json::Value = serde_json::from_str(&response).expect("response");
    let mut context: QualificationDispatch =
        serde_json::from_value(response_value["plan"]["qualification"].clone())
            .expect("qualification context");
    context.campaign = "other-campaign".to_owned();
    let snapshot = crate::qualification_cache_backend::complete_snapshot_for_test(&context)
        .expect("foreign snapshot");
    crate::qualification_cache_backend::stage_qualification_cache_before(
        artifacts.path(),
        &snapshot,
    )
    .expect("stage foreign snapshot");
    let error =
        publish_plan_files_with_outputs(&response, &artifacts.path().join("velnor"), &outputs)
            .expect_err("snapshot binding must match final plan");
    assert!(matches!(error, OrchestratorError::Internal { .. }));
}

fn cold_qualification_response() -> String {
    let run_key = run_key_for_ci(123, 1);
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan ID"),
        base: None,
        head: SOURCE.to_owned(),
        event: WorkflowEvent::Qualification,
        qualification: Some(QualificationDispatch {
            mode: velnor_actions_contract::QualificationDispatchMode::Controlled,
            campaign: "artifact-test".to_owned(),
            phase: QualificationPhase::Cold,
            repository: "owner/project".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: SOURCE.to_owned(),
            source_sha: SOURCE.to_owned(),
            run_id: 123,
            run_attempt: 1,
            predecessor: None,
            scale_set_image_source_sha: None,
        }),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: Vec::new(),
        jobs: Vec::new(),
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    let matrix = plan.matrix.clone();
    serde_json::to_string(&serde_json::json!({
        "schema": 1,
        "plan": plan,
        "matrix": matrix,
    }))
    .expect("serialized plan response")
}
