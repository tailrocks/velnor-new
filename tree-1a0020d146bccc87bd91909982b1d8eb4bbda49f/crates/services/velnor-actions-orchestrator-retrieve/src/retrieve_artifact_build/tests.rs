#[test]
fn build_artifact_download_name_uses_typed_identity_family() {
    let expected = expectation();
    let name = expected_build_artifact_name(&expected.identity).expect("typed build name");
    assert_eq!(name, "velnor-build-7-1-hosted-bundle");

    let mut invalid = expected.identity;
    invalid.task_id = "../bundle".to_owned();
    assert_eq!(
        expected_build_artifact_name(&invalid),
        Err("artifact_name_invalid")
    );
}

use super::*;

use velnor_actions_contract_config::{ArtifactBuildOutput, ArtifactBuildTask, VerificationRunner};

fn task() -> ArtifactBuildTask {
    ArtifactBuildTask {
        id: "bundle".to_owned(),
        mise_task: "build-bundle".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 15,
        outputs: vec![ArtifactBuildOutput {
            id: "archive".to_owned(),
            path: "dist/archive.tar".to_owned(),
            max_bytes: 64,
        }],
    }
}

fn context() -> ArtifactBuildRunContext {
    ArtifactBuildRunContext {
        repository_id: "123".to_owned(),
        repository: "owner/repo".to_owned(),
        run_id: "7".to_owned(),
        run_attempt: 1,
    }
}

fn expectation() -> ArtifactBuildExpectation {
    let task = task();
    ArtifactBuildExpectation {
        identity: ArtifactBuildIdentity {
            repository_id: "123".to_owned(),
            repository: "owner/repo".to_owned(),
            source_sha: "a".repeat(40),
            plan_digest: format!("b3-{}", "0".repeat(64)),
            run_id: "7".to_owned(),
            run_attempt: 1,
            workflow_job_id: "artifact-build".to_owned(),
            provider: ArtifactBuildProvider::GithubHosted,
            task_id: task.id,
        },
        outputs: task.outputs,
    }
}

fn api_job(expected: &ArtifactBuildExpectation) -> ApiJob {
    ApiJob {
        id: 70,
        run_id: 7,
        head_sha: expected.identity.source_sha.clone(),
        status: "completed".to_owned(),
        conclusion: Some("success".to_owned()),
        name: expected
            .identity
            .provider
            .workflow_job_name(&expected.identity.task_id),
        check_run_url: None,
        check_run_id: None,
        runner_id: None,
        runner_name: None,
        runner_group_id: None,
        runner_group_name: None,
        labels: vec!["ubuntu-26.04".to_owned()],
    }
}

fn api_artifact(expected: &ArtifactBuildExpectation) -> ApiArtifact {
    ApiArtifact {
        id: 700,
        name: velnor_actions_contract_workflow::artifact_name(&expected.identity)
            .expect("artifact name"),
        size_in_bytes: 4096,
        expired: false,
        workflow_run: Some(api::ApiArtifactRun {
            id: 7,
            repository_id: 123,
            head_sha: expected.identity.source_sha.clone(),
        }),
    }
}

#[test]
fn api_inventory_is_exact_for_current_attempt_and_expected_task_names() {
    let expected = expectation();
    let job = api_job(&expected);
    let artifact = api_artifact(&expected);
    validate_api_inventory(
        std::slice::from_ref(&job),
        std::slice::from_ref(&artifact),
        std::slice::from_ref(&expected),
        7,
        1,
    )
    .expect("one expected pair");

    let mut extra_job = job.clone();
    extra_job.name.push_str(" / unexpected");
    assert_eq!(
        validate_api_inventory(
            &[job.clone(), extra_job],
            std::slice::from_ref(&artifact),
            std::slice::from_ref(&expected),
            7,
            1
        )
        .expect_err("unexpected build job"),
        "artifact_api_unexpected_or_duplicate_job"
    );
    assert_eq!(
        validate_api_inventory(
            std::slice::from_ref(&job),
            &[artifact.clone(), artifact.clone()],
            &[expected],
            7,
            1
        )
        .expect_err("duplicate artifact name"),
        "artifact_api_unexpected_or_duplicate_artifact"
    );
}

#[test]
fn observation_keeps_missing_job_and_artifact_fields_instead_of_inventing_success() {
    let expected = expectation();
    let missing = observation_for(&expected, None, None);
    assert_eq!(missing.api_job_id, None);
    assert_eq!(missing.api_artifact_id, None);
    assert_eq!(missing.conclusion, JobConclusion::Missing);
    assert!(!download_is_bound_to_expected(
        &missing,
        &context(),
        &expected,
        &task()
    ));

    let mut wrong_source = observation_for(
        &expected,
        Some(&api_job(&expected)),
        Some(&api_artifact(&expected)),
    );
    wrong_source.api_artifact_head_sha = Some("b".repeat(40));
    assert!(!download_is_bound_to_expected(
        &wrong_source,
        &context(),
        &expected,
        &task()
    ));
}

#[test]
fn exact_archive_download_argv_pins_repository_in_endpoint_and_artifact_id() {
    let args = archive_download_args("owner/repo", 700);
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["api", "repos/owner/repo/actions/artifacts/700/zip",]);
    assert_eq!(
        maximum_artifact_size(&task()),
        Some(64 + download::MAX_RESULT_BYTES + 1024 + 7)
    );
}
