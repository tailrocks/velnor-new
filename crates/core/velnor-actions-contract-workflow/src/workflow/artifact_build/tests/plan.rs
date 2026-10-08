use super::*;

#[test]
fn plan_rejects_unsorted_or_colliding_artifact_task_ids() {
    let mut unsorted = plan();
    let mut earlier = task();
    earlier.id = "another-bundle".to_owned();
    unsorted.artifact_tasks.push(ArtifactBuildTaskPlan {
        task: earlier,
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
