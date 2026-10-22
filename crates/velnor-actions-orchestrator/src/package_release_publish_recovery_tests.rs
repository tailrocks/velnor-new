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
    assert_eq!(
        validate_artifact(&artifact, 42, 9, "main").expect("original artifact source"),
        "a".repeat(40)
    );
    assert!(validate_artifact(&artifact, 42, 10, "main").is_err());
    assert!(validate_artifact(&artifact, 42, 9, "release").is_err());
}

#[test]
fn source_selection_and_finalization_use_distinct_fixed_staging_directories() {
    let runner_root = tempfile::tempdir().expect("runner temp root");
    let selection =
        PreviewRecoveryVerificationStage::SourceSelection.input_directory(runner_root.path());
    let finalization =
        PreviewRecoveryVerificationStage::Finalization.input_directory(runner_root.path());

    assert_ne!(selection, finalization);
    create_fresh_directory(&selection).expect("source selection stage directory");
    create_fresh_directory(&finalization).expect("finalization stage directory");
    assert!(selection.is_dir());
    assert!(finalization.is_dir());
}

#[test]
fn recovery_binds_receipt_to_original_artifact_not_current_dispatch_head_or_count() {
    let original_sha = "a".repeat(40);
    let current_dispatch_sha = "b".repeat(40);
    let original_count = 17;
    let current_dispatch_count = 23;
    assert_ne!(original_sha, current_dispatch_sha);
    assert_ne!(original_count, current_dispatch_count);

    assert!(receipt_matches_original(
        42,
        2,
        &original_sha,
        (PackageReleaseChannel::Preview, Some(original_count)),
        42,
        2,
        &original_sha,
    ));
    assert!(!receipt_matches_original(
        41,
        2,
        &original_sha,
        (PackageReleaseChannel::Preview, Some(original_count)),
        42,
        2,
        &original_sha,
    ));
    assert!(!receipt_matches_original(
        42,
        1,
        &original_sha,
        (PackageReleaseChannel::Preview, Some(original_count)),
        42,
        2,
        &original_sha,
    ));
    assert!(!receipt_matches_original(
        42,
        2,
        &current_dispatch_sha,
        (PackageReleaseChannel::Preview, Some(original_count)),
        42,
        2,
        &original_sha,
    ));
    assert!(!receipt_matches_original(
        42,
        2,
        &original_sha,
        (PackageReleaseChannel::Preview, Some(0)),
        42,
        2,
        &original_sha,
    ));
}

#[test]
fn recovery_original_run_must_match_exact_workflow_source_and_attempt() {
    let plan = preview_plan();
    let source_sha = "a".repeat(40);
    let authority_sha = "b".repeat(40);
    assert_ne!(source_sha, authority_sha);
    let run = WorkflowRunResponse {
        id: Some(42),
        run_attempt: Some(2),
        path: Some(".github/workflows/binary-preview.yml".to_owned()),
        head_sha: Some(authority_sha.clone()),
        head_branch: Some("main".to_owned()),
        event: Some("workflow_run".to_owned()),
        name: Some("Rust binary preview".to_owned()),
        workflow_id: Some(7),
        run_number: Some(12),
        head_repository: Some(RunRepository { id: 9 }),
        status: Some("completed".to_owned()),
        conclusion: Some("failure".to_owned()),
        repository: Some(RunRepository { id: 9 }),
    };
    let workflow = WorkflowMetadata {
        state: "active".to_owned(),
        id: 7,
        path: plan.channel.workflow_path().to_owned(),
        name: plan.channel.workflow_name().to_owned(),
    };
    assert!(source_run_matches(
        &run,
        &plan,
        &authority_sha,
        42,
        2,
        9,
        &workflow
    ));
    assert!(receipt_matches_original(
        42,
        2,
        &source_sha,
        (PackageReleaseChannel::Preview, Some(17)),
        42,
        2,
        &source_sha,
    ));
    assert!(!source_run_matches(
        &run,
        &plan,
        &source_sha,
        42,
        2,
        9,
        &workflow
    ));
    assert!(!source_run_matches(
        &run,
        &plan,
        &authority_sha,
        42,
        1,
        9,
        &workflow
    ));
    assert!(!source_run_matches(
        &run,
        &plan,
        &authority_sha,
        42,
        2,
        10,
        &workflow
    ));
}

#[test]
fn recovery_rejects_original_workflow_path_mismatch() {
    let plan = preview_plan();
    let sha = "b".repeat(40);
    let run: WorkflowRunResponse = serde_json::from_value(serde_json::json!({
        "id":42,"run_attempt":2,"run_number":12,"workflow_id":7,
        "path":".github/workflows/binary-release.yml","name":"Rust binary preview",
        "head_sha":sha,"head_branch":"main","event":"workflow_run",
        "status":"completed","conclusion":"failure",
        "repository":{"id":9},"head_repository":{"id":9}
    }))
    .expect("run fixture");
    let workflow = WorkflowMetadata {
        state: "active".to_owned(),
        id: 7,
        path: plan.channel.workflow_path().to_owned(),
        name: plan.channel.workflow_name().to_owned(),
    };
    assert!(!source_run_matches(&run, &plan, &sha, 42, 2, 9, &workflow));
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

#[test]
fn recovery_rejects_original_push_or_dispatch_instead_of_workflow_run() {
    let plan = preview_plan();
    let workflow = WorkflowMetadata {
        state: "active".to_owned(),
        id: 7,
        path: plan.channel.workflow_path().to_owned(),
        name: plan.channel.workflow_name().to_owned(),
    };
    let sha = "a".repeat(40);
    for event in ["push", "workflow_dispatch", "pull_request"] {
        let run: WorkflowRunResponse = serde_json::from_value(serde_json::json!({
            "id":42,"run_attempt":2,"run_number":12,"workflow_id":7,
            "path":workflow.path,"name":workflow.name,"head_sha":sha,
            "head_branch":"main","event":event,"status":"completed","conclusion":"failure",
            "repository":{"id":9},"head_repository":{"id":9}
        }))
        .expect("run fixture");
        assert!(!source_run_matches(&run, &plan, &sha, 42, 2, 9, &workflow));
    }
}

#[test]
fn recovery_stable_identity_requires_no_preview_count_and_exact_original() {
    let sha = "a".repeat(40);
    assert!(receipt_matches_original(
        42,
        2,
        &sha,
        (PackageReleaseChannel::Stable, None),
        42,
        2,
        &sha
    ));
    for count in [Some(0), Some(17)] {
        assert!(!receipt_matches_original(
            42,
            2,
            &sha,
            (PackageReleaseChannel::Stable, count),
            42,
            2,
            &sha
        ));
    }
    assert!(!receipt_matches_original(
        42,
        2,
        &sha,
        (PackageReleaseChannel::Stable, None),
        42,
        3,
        &sha
    ));
    assert!(!receipt_matches_original(
        42,
        2,
        &sha,
        (PackageReleaseChannel::Stable, None),
        42,
        2,
        &"b".repeat(40)
    ));
}
