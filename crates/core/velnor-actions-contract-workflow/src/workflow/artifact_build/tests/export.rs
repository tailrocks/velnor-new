use super::*;

#[test]
fn output_export_is_bound_to_run_attempt_source_provider_task_and_plan() {
    let plan = plan();
    plan.validate().expect("valid authoritative plan");
    let task = &plan.artifact_tasks.first().expect("planned task").task;
    let expected = expected_artifact_builds(
        &plan,
        &run_context(),
        &[
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    )
    .expect("provider/task obligations from plan");
    assert!(
        expected_artifact_builds(
            &plan,
            &ArtifactBuildRunContext {
                run_attempt: 3,
                ..run_context()
            },
            &[
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        )
        .is_err()
    );
    let hosted = observation(task, expected[0].identity.clone());
    let velnor = observation(task, expected[1].identity.clone());

    assert_ne!(
        hosted.result.as_ref().expect("hosted result").artifact_name,
        velnor.result.as_ref().expect("Velnor result").artifact_name,
    );
    let serialized =
        serde_json::to_vec(hosted.result.as_ref().expect("result")).expect("serialize result");
    let parsed: ArtifactBuildResult = serde_json::from_slice(&serialized).expect("parse result");
    parsed
        .validate_for(task, &expected[0].identity)
        .expect("round-trip preserves identity");

    reconcile_artifact_builds(
        &plan,
        &run_context(),
        &[
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
        &[hosted.clone(), velnor.clone()],
    )
    .expect("exact successful provider pair reconciles");

    let mut tampered = hosted.clone();
    tampered.downloaded_outputs[0].digest =
        velnor_actions_contract::digest_b3(b"tampered downloaded output");
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &[
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
            &[tampered, velnor.clone()],
        )
        .is_err()
    );
}

#[test]
fn export_rejects_empty_oversize_and_incomplete_outputs() {
    let task = task();
    let digest = "b3-0000000000000000000000000000000000000000000000000000000000000000";
    let expected_identity = identity(ArtifactBuildProvider::GithubHosted, digest);
    assert!(
        export_artifact_result(
            expected_identity.clone(),
            &task,
            &[("bundle".into(), vec![])]
        )
        .is_err()
    );
    assert!(
        export_artifact_result(
            expected_identity.clone(),
            &task,
            &[("bundle".into(), vec![b'x'; 1025])],
        )
        .is_err()
    );
    assert!(export_artifact_result(expected_identity, &task, &[]).is_err());
}

#[test]
fn artifact_names_include_run_attempt_provider_and_task() {
    let digest = "b3-0000000000000000000000000000000000000000000000000000000000000000";
    let name = artifact_name(&identity(ArtifactBuildProvider::VelnorScaleSet, digest))
        .expect("valid identity");
    assert_eq!(name, "velnor-build-123456789-2-velnor-frontend-bundle");
}

#[test]
fn legacy_schema_one_plan_defaults_to_an_omitted_empty_artifact_inventory() {
    let mut legacy_value = serde_json::to_value(plan()).expect("serialize fixture plan");
    legacy_value
        .as_object_mut()
        .expect("plan object")
        .remove("artifact_tasks");

    let legacy_plan: Plan = serde_json::from_value(legacy_value).expect("legacy plan parses");
    assert_eq!(legacy_plan.artifact_tasks, Vec::new());
    legacy_plan.validate().expect("legacy plan remains valid");
    assert!(
        serde_json::to_value(&legacy_plan)
            .expect("serialize legacy plan")
            .get("artifact_tasks")
            .is_none(),
        "empty optional field does not change existing Schema 1 bytes"
    );
    assert_eq!(
        expected_artifact_builds(
            &legacy_plan,
            &run_context(),
            &[ArtifactBuildProvider::GithubHosted],
        )
        .expect("no artifact obligations"),
        Vec::new()
    );
}
