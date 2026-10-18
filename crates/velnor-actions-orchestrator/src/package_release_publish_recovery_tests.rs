use super::*;
use crate::package_release_publish::github::{RunRepository, WorkflowJob};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, package_release::PackageReleaseChannel,
};

fn preview_plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: Vec::new(),
        companions: Vec::new(),
    }
}

fn successful_signer_job() -> WorkflowJob {
    WorkflowJob {
        run_id: 42,
        name: "preview-signer".to_owned(),
        head_sha: Some("a".repeat(40)),
        status: Some("completed".to_owned()),
        conclusion: Some("success".to_owned()),
    }
}

#[test]
fn recovery_requires_one_successful_signer_job_for_original_attempt_sha() {
    let job = successful_signer_job();
    assert!(
        require_successful_job(
            &WorkflowJobList {
                jobs: vec![job.clone()]
            },
            42,
            &"a".repeat(40),
            "preview-signer"
        )
        .is_ok()
    );

    assert!(
        require_successful_job(
            &WorkflowJobList { jobs: vec![] },
            42,
            &"a".repeat(40),
            "preview-signer"
        )
        .is_err()
    );
    assert!(
        require_successful_job(
            &WorkflowJobList {
                jobs: vec![job.clone(), job.clone()]
            },
            42,
            &"a".repeat(40),
            "preview-signer"
        )
        .is_err()
    );

    let mut wrong_run = job.clone();
    wrong_run.run_id = 41;
    assert!(
        require_successful_job(
            &WorkflowJobList {
                jobs: vec![wrong_run]
            },
            42,
            &"a".repeat(40),
            "preview-signer"
        )
        .is_err()
    );
    let mut wrong_head = job;
    wrong_head.head_sha = Some("b".repeat(40));
    assert!(
        require_successful_job(
            &WorkflowJobList {
                jobs: vec![wrong_head]
            },
            42,
            &"a".repeat(40),
            "preview-signer"
        )
        .is_err()
    );
}

#[test]
fn recovery_artifact_must_belong_to_exact_repo_sha_and_default_branch() {
    let artifact = WorkflowArtifact {
        id: 7,
        name: "velnor-preview-signed-42-1".to_owned(),
        expired: false,
        size_in_bytes: 100,
        digest: None,
        workflow_run: Some(crate::package_release_publish::github::ArtifactRun {
            id: 42,
            head_sha: "a".repeat(40),
            head_branch: "main".to_owned(),
            repository_id: 9,
        }),
    };
    assert!(validate_artifact(&artifact, 42, 9, "main", &"a".repeat(40)).is_ok());
    assert!(validate_artifact(&artifact, 42, 10, "main", &"a".repeat(40)).is_err());
    assert!(validate_artifact(&artifact, 42, 9, "main", &"b".repeat(40)).is_err());
    assert!(validate_artifact(&artifact, 42, 9, "release", &"a".repeat(40)).is_err());
}

#[test]
fn recovery_original_run_must_match_exact_workflow_source_and_attempt() {
    let plan = preview_plan();
    let sha = "a".repeat(40);
    let run = WorkflowRunResponse {
        id: Some(42),
        run_attempt: Some(2),
        path: Some(".github/workflows/binary-preview.yml".to_owned()),
        head_sha: Some(sha.clone()),
        head_branch: Some("main".to_owned()),
        event: Some("push".to_owned()),
        status: Some("completed".to_owned()),
        conclusion: Some("failure".to_owned()),
        repository: Some(RunRepository { id: 9 }),
    };
    assert!(source_run_matches(&run, &plan, &sha, 42, 2, 9));
    assert!(!source_run_matches(&run, &plan, &sha, 42, 1, 9));
    assert!(!source_run_matches(&run, &plan, &sha, 42, 2, 10));

    let mut wrong_path = run;
    wrong_path.path = Some(".github/workflows/binary-release.yml".to_owned());
    assert!(!source_run_matches(&wrong_path, &plan, &sha, 42, 2, 9));
}

#[test]
fn recovery_rejects_a_retained_zip_without_the_signed_receipt() {
    let mut bytes = Vec::new();
    zip::ZipWriter::new(std::io::Cursor::new(&mut bytes))
        .finish()
        .expect("empty zip archive");
    let scratch = tempfile::tempdir().expect("temporary output root");
    assert!(
        archive::extract_signed_assets(&bytes, scratch.path(), &preview_plan()).is_err(),
        "artifact admission requires a receipt before accepting any retained bytes"
    );
}
