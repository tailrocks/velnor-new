use super::*;

#[test]
fn reconciliation_rejects_missing_duplicate_failed_and_mismatched_results() {
    let plan = plan();
    let task = &plan.artifact_tasks.first().expect("planned task").task;
    let providers = [
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::VelnorScaleSet,
    ];
    let expected = expected_artifact_builds(&plan, &run_context(), &providers)
        .expect("expected plan inventory");
    let hosted = observation(task, expected[0].identity.clone());
    let velnor = observation(task, expected[1].identity.clone());

    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            std::slice::from_ref(&hosted),
        )
        .is_err()
    );
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[hosted.clone(), hosted.clone()],
        )
        .is_err()
    );

    let mut failed = velnor.clone();
    failed.conclusion = JobConclusion::Skipped;
    assert!(
        reconcile_artifact_builds(&plan, &run_context(), &providers, &[hosted.clone(), failed],)
            .is_err()
    );

    let mut result_missing = velnor.clone();
    result_missing.result = None;
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[hosted.clone(), result_missing],
        )
        .is_err()
    );

    let mut result_identity_mismatch = velnor.clone();
    result_identity_mismatch
        .result
        .as_mut()
        .expect("existing Velnor result")
        .identity
        .source_sha = "b4dfd62241cf85172a3c34ca5ab5e1750907d053".to_owned();
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[hosted.clone(), result_identity_mismatch],
        )
        .is_err()
    );

    let mut mismatched = velnor.clone();
    mismatched.identity.run_attempt += 1;
    assert!(
        reconcile_artifact_builds(&plan, &run_context(), &providers, &[hosted, mismatched],)
            .is_err()
    );
}

#[test]
fn reconciliation_requires_unique_actions_api_job_and_artifact_identity() {
    let plan = plan();
    let task = &plan.artifact_tasks[0].task;
    let providers = [
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::VelnorScaleSet,
    ];
    let expected = expected_artifact_builds(&plan, &run_context(), &providers)
        .expect("expected plan inventory");
    let hosted = observation(task, expected[0].identity.clone());
    let velnor = observation(task, expected[1].identity.clone());

    for (invalid, code) in [
        (
            {
                let mut value = hosted.clone();
                value.api_job_id = Some(0);
                value
            },
            "missing_api_job_id",
        ),
        (
            {
                let mut value = hosted.clone();
                value.api_job_name.push_str(" wrong");
                value
            },
            "api_job_or_artifact_name_mismatch",
        ),
        (
            {
                let mut value = hosted.clone();
                value.api_artifact_id = Some(0);
                value
            },
            "missing_api_artifact_id",
        ),
        (
            {
                let mut value = hosted.clone();
                value
                    .api_artifact_name
                    .as_mut()
                    .expect("artifact name")
                    .push_str("-wrong");
                value
            },
            "api_job_or_artifact_name_mismatch",
        ),
        (
            {
                let mut value = hosted.clone();
                value.api_artifact_size_bytes = Some(0);
                value
            },
            "missing_or_duplicate_api_identity",
        ),
    ] {
        let error = reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[invalid, velnor.clone()],
        )
        .expect_err("unverified Actions API metadata must fail closed");
        assert!(error.to_string().contains(code), "{error}");
    }

    let mut duplicate_api_job = velnor.clone();
    duplicate_api_job.api_job_id = hosted.api_job_id;
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[hosted.clone(), duplicate_api_job],
        )
        .is_err()
    );
    let mut duplicate_api_artifact = velnor.clone();
    duplicate_api_artifact.api_artifact_id = hosted.api_artifact_id;
    assert!(
        reconcile_artifact_builds(
            &plan,
            &run_context(),
            &providers,
            &[hosted, duplicate_api_artifact],
        )
        .is_err()
    );
}
