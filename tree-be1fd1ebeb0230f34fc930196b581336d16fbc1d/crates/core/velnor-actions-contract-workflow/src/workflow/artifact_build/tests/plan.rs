use super::*;

#[test]
fn plan_rejects_unsorted_or_colliding_artifact_task_ids() {
    let mut unsorted = plan();
    let mut earlier = task();
    earlier.id = "another-bundle".to_owned();
    unsorted.artifact_tasks.push(ArtifactBuildTaskPlan {
        task: earlier,
        producer: ArtifactBuildProducer::MatrixBuild,
        providers: vec![
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    });
    let error = unsorted
        .validate()
        .expect_err("unsorted inventory is invalid");
    assert!(
        error
            .to_string()
            .contains("artifact_tasks_must_be_sorted_by_id")
    );

    let mut collision = plan();
    collision.task_ids.push("frontend-bundle".to_owned());
    collision.task_ids.sort();
    let error = collision
        .validate()
        .expect_err("artifact tasks cannot reuse an obligation ID");
    assert!(
        error
            .to_string()
            .contains("task_id_collision:frontend-bundle")
    );
}

#[test]
fn plan_rejects_per_task_provider_scope_drift() {
    let mut plan = plan();
    let mut differently_routed = task();
    differently_routed.id = "frontend-paired".to_owned();
    differently_routed.mise_task = "build-frontend-paired".to_owned();
    plan.artifact_tasks.push(ArtifactBuildTaskPlan {
        task: differently_routed,
        producer: ArtifactBuildProducer::MatrixBuild,
        providers: vec![ArtifactBuildProvider::GithubHosted],
    });
    assert!(
        plan.validate()
            .is_err_and(|error| error.to_string().contains("provider_scope_mismatch"))
    );
}

#[test]
fn provider_matrices_are_canonical_and_never_mix_lanes() {
    let plan = plan();
    let hosted = artifact_matrix_for_provider(&plan, ArtifactBuildProvider::GithubHosted)
        .expect("hosted artifact matrix");
    let velnor = artifact_matrix_for_provider(&plan, ArtifactBuildProvider::VelnorScaleSet)
        .expect("Velnor artifact matrix");

    let hosted_matrix: ArtifactBuildMatrix =
        serde_json::from_str(&hosted).expect("hosted matrix shape");
    let velnor_matrix: ArtifactBuildMatrix =
        serde_json::from_str(&velnor).expect("Velnor matrix shape");
    assert_eq!(hosted_matrix.include.len(), 1);
    assert_eq!(velnor_matrix.include.len(), 1);
    assert_eq!(
        hosted_matrix.include[0].provider,
        ArtifactBuildProvider::GithubHosted
    );
    assert_eq!(
        velnor_matrix.include[0].provider,
        ArtifactBuildProvider::VelnorScaleSet
    );
    assert_eq!(hosted_matrix.include[0].task_id, "frontend-bundle");
    assert_eq!(velnor_matrix.include[0].task_id, "frontend-bundle");
    assert_eq!(hosted_matrix.include[0].source_sha, plan.head);
    assert_eq!(
        hosted_matrix.include[0].plan_digest,
        velnor_matrix.include[0].plan_digest
    );
    assert_eq!(hosted_matrix.include[0].mise_task, "build-frontend");
    assert_eq!(hosted_matrix.include[0].outputs, task().outputs);
    assert!(hosted.starts_with("{\"include\":[{"));
    assert!(velnor.starts_with("{\"include\":[{"));
}

#[test]
fn mixed_producers_share_expected_identity_but_only_matrix_tasks_enter_matrices() {
    let mut plan = plan();
    let mut static_task = task();
    static_task.id = "verification-output".to_owned();
    static_task.mise_task = "write-verification-output".to_owned();
    plan.artifact_tasks.push(ArtifactBuildTaskPlan {
        task: static_task,
        producer: ArtifactBuildProducer::VerificationTask,
        providers: vec![
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    });
    plan.validate().expect("mixed producer plan is valid");

    let providers = [
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::VelnorScaleSet,
    ];
    let expected = expected_artifact_builds(&plan, &run_context(), &providers)
        .expect("both producer classes have exact identities");
    assert_eq!(expected.len(), 4);
    let hosted_static = expected
        .iter()
        .find(|item| {
            item.producer == ArtifactBuildProducer::VerificationTask
                && item.identity.provider == ArtifactBuildProvider::GithubHosted
        })
        .expect("hosted static producer");
    assert_eq!(
        hosted_static.identity.workflow_job_id,
        "task-verification-output__hosted"
    );
    assert_eq!(
        hosted_static.expected_workflow_job_name(),
        "Verify verification-output / GitHub hosted / Linux x64"
    );
    let scale_static = expected
        .iter()
        .find(|item| {
            item.producer == ArtifactBuildProducer::VerificationTask
                && item.identity.provider == ArtifactBuildProvider::VelnorScaleSet
        })
        .expect("Scale Set static producer");
    assert_eq!(
        scale_static.identity.workflow_job_id,
        "task-verification-output__local"
    );
    assert_eq!(
        scale_static.expected_workflow_job_name(),
        "Verify verification-output / Velnor Scale Set / Linux x64"
    );

    for provider in providers {
        let matrix = artifact_matrix_for_provider(&plan, provider).expect("matrix JSON");
        let matrix: ArtifactBuildMatrix = serde_json::from_str(&matrix).expect("matrix shape");
        assert_eq!(matrix.include.len(), 1);
        assert_eq!(matrix.include[0].task_id, "frontend-bundle");
    }
}

#[test]
fn per_task_provider_scopes_form_one_complete_inventory_without_changing_matrix_scope() {
    let mut plan = plan();
    let providers = [
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::VelnorScaleSet,
    ];
    for (id, task_providers) in [
        ("verify-both", providers.to_vec()),
        ("verify-hosted", vec![ArtifactBuildProvider::GithubHosted]),
        ("verify-scale", vec![ArtifactBuildProvider::VelnorScaleSet]),
    ] {
        let mut task = task();
        task.id = id.to_owned();
        task.mise_task = format!("check-{id}");
        plan.artifact_tasks.push(ArtifactBuildTaskPlan {
            task,
            producer: ArtifactBuildProducer::VerificationTask,
            providers: task_providers,
        });
    }

    plan.validate()
        .expect("static tasks have independent scopes");
    assert_eq!(artifact_plan_providers(&plan), providers);
    let expected = expected_artifact_builds(&plan, &run_context(), &providers)
        .expect("complete union derives every task/provider pair");
    assert_eq!(expected.len(), 6);
    for (task_id, providers_for_task) in [
        ("frontend-bundle", providers.as_slice()),
        ("verify-both", providers.as_slice()),
        ("verify-hosted", &[ArtifactBuildProvider::GithubHosted][..]),
        ("verify-scale", &[ArtifactBuildProvider::VelnorScaleSet][..]),
    ] {
        let actual: Vec<_> = expected
            .iter()
            .filter(|item| item.identity.task_id == task_id)
            .map(|item| item.identity.provider)
            .collect();
        assert_eq!(actual, providers_for_task, "task {task_id}");
    }
    assert!(
        expected_artifact_builds(&plan, &run_context(), &[providers[0]])
            .is_err_and(|error| error.to_string().contains("provider_scope_mismatch"))
    );

    for provider in providers {
        let matrix = artifact_matrix_for_provider(&plan, provider).expect("matrix JSON");
        let matrix: ArtifactBuildMatrix = serde_json::from_str(&matrix).expect("matrix shape");
        assert_eq!(matrix.include.len(), 1);
        assert_eq!(matrix.include[0].task_id, "frontend-bundle");
    }
}

#[test]
fn mixed_plan_still_rejects_duplicate_tasks_outputs_and_unknown_tokens() {
    let mut duplicate_task = plan();
    duplicate_task.artifact_tasks.push(ArtifactBuildTaskPlan {
        task: task(),
        producer: ArtifactBuildProducer::VerificationTask,
        providers: vec![ArtifactBuildProvider::GithubHosted],
    });
    assert!(
        duplicate_task
            .validate()
            .is_err_and(|error| error.to_string().contains("duplicate_artifact_task"))
    );

    let mut duplicate_output = mixed_plan();
    let duplicate_output_declaration = duplicate_output.artifact_tasks[1].task.outputs[0].clone();
    duplicate_output.artifact_tasks[1]
        .task
        .outputs
        .push(duplicate_output_declaration);
    assert!(
        duplicate_output
            .validate()
            .is_err_and(|error| error.to_string().contains("duplicate_artifact_output"))
    );

    let mut unknown_provider = serde_json::to_value(mixed_plan()).expect("plan value");
    unknown_provider["artifact_tasks"][1]["providers"][0] = serde_json::json!("untrusted");
    assert!(serde_json::from_value::<Plan>(unknown_provider).is_err());

    let mut unknown_producer = serde_json::to_value(mixed_plan()).expect("plan value");
    unknown_producer["artifact_tasks"][1]["producer"] = serde_json::json!("untrusted");
    assert!(serde_json::from_value::<Plan>(unknown_producer).is_err());
}

#[test]
fn static_producer_scopes_are_independently_nonempty_and_unique() {
    let mut empty_scope = mixed_plan();
    empty_scope.artifact_tasks[1].providers.clear();
    assert!(
        empty_scope
            .validate()
            .is_err_and(|error| error.to_string().contains("empty_provider_inventory"))
    );

    let mut duplicate_scope = mixed_plan();
    duplicate_scope.artifact_tasks[1].providers = vec![
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::GithubHosted,
    ];
    assert!(duplicate_scope.validate().is_err_and(|error| {
        error
            .to_string()
            .contains("providers_must_be_sorted_unique")
    }));
}

#[test]
fn legacy_matrix_wire_shapes_omit_new_discriminators_and_still_deserialize() {
    let plan = plan();
    let old_plan_value = serde_json::to_value(&plan).expect("serialize plan");
    let old_task = &old_plan_value["artifact_tasks"][0];
    assert!(old_task.get("producer").is_none());
    let decoded_plan: Plan = serde_json::from_value(old_plan_value).expect("old plan shape");
    assert_eq!(
        decoded_plan.artifact_tasks[0].producer,
        ArtifactBuildProducer::MatrixBuild
    );

    let expected = expected_artifact_builds(
        &plan,
        &run_context(),
        &[
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    )
    .expect("legacy expected inventory");
    let old_expected_value = serde_json::to_value(&expected[0]).expect("serialize expectation");
    assert!(old_expected_value.get("producer").is_none());
    assert!(old_expected_value.get("workflow_job_name").is_none());
    let decoded: ArtifactBuildExpectation =
        serde_json::from_value(old_expected_value).expect("old expectation shape");
    assert_eq!(decoded.producer, ArtifactBuildProducer::MatrixBuild);
    assert_eq!(
        decoded.expected_workflow_job_name(),
        "Build artifact github_hosted / frontend-bundle"
    );
}
