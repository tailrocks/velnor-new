use super::*;
use crate::mise_task_suite_artifact_name_for_attempt;

fn source() -> GitHubEvidenceArtifactRefV1 {
    GitHubEvidenceArtifactRefV1 {
        suite_id: "producer".to_owned(),
        artifact_name: "ledger".to_owned(),
        workflow_path: ".github/workflows/producer.yml".to_owned(),
        job_id: "mise-task-producer".to_owned(),
        job_name: "Mise task producer".to_owned(),
    }
}

fn spec() -> GitHubEvidenceFetchSpecV1 {
    GitHubEvidenceFetchSpecV1 {
        schema: GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
        collector_suite_id: "collector".to_owned(),
        collector_workflow_path: ".github/workflows/nightly.yml".to_owned(),
        default_branch: "main".to_owned(),
        window_days: 31,
        workflow_sources: vec![
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::PrimaryCi,
                path: ".github/workflows/ci.yml".to_owned(),
                job_id: None,
                job_name: None,
            },
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::TaskSuite {
                    suite_id: "producer".to_owned(),
                },
                path: ".github/workflows/producer.yml".to_owned(),
                job_id: Some("mise-task-producer".to_owned()),
                job_name: Some("Mise task producer".to_owned()),
            },
        ],
        artifact_sources: vec![source()],
    }
}

fn attempt(
    attempt: u32,
    is_predecessor: bool,
    conclusion: Option<&str>,
) -> GitHubEvidenceRunAttemptV1 {
    GitHubEvidenceRunAttemptV1 {
        workflow_path: ".github/workflows/producer.yml".to_owned(),
        run_id: if is_predecessor { 99 } else { 100 },
        attempt,
        created_at: if is_predecessor {
            "2026-09-01T00:00:00Z".to_owned()
        } else {
            "2026-10-22T00:00:00Z".to_owned()
        },
        is_predecessor,
        event: "push".to_owned(),
        head_branch: "main".to_owned(),
        head_sha: "a".repeat(40),
        status: "completed".to_owned(),
        conclusion: conclusion.map(str::to_owned),
        jobs_documents: vec![1],
    }
}

fn metadata_document(index: u64, run_id: u64, attempt: u32) -> GitHubEvidenceDocumentV1 {
    GitHubEvidenceDocumentV1 {
        kind: GitHubEvidenceDocumentKindV1::ArtifactById,
        workflow_path: Some(".github/workflows/producer.yml".to_owned()),
        run_id: Some(run_id),
        attempt: Some(attempt),
        artifact_id: Some(index),
        page: None,
        path: format!("documents/artifact-{index}.json"),
        size_bytes: 1,
        sha256: "0".repeat(64),
    }
}

fn artifact(index: u64, attempt: u32) -> GitHubEvidenceArtifactCaptureV1 {
    GitHubEvidenceArtifactCaptureV1 {
        source: source(),
        artifact_id: index,
        run_id: 100,
        name: mise_task_suite_artifact_name_for_attempt("producer", "ledger", 100, attempt),
        path: format!("archives/{index}.zip"),
        metadata_document: attempt - 1,
        size_bytes: 4,
        sha256: "1".repeat(64),
    }
}

fn snapshot() -> GitHubEvidenceSnapshotV1 {
    GitHubEvidenceSnapshotV1 {
        schema: GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
        spec_sha256: "2".repeat(64),
        repository: GitHubEvidenceRepositoryV1 {
            id: 1,
            owner: "tailrocks".to_owned(),
            name: "velnor".to_owned(),
            default_branch: "main".to_owned(),
        },
        acquisition: GitHubEvidenceAcquisitionV1 {
            run_id: 1,
            run_attempt: 1,
            collector_ref: "refs/heads/main".to_owned(),
            collector_sha: "a".repeat(40),
            captured_at: "2026-10-23T00:00:00Z".to_owned(),
            window_start: "2026-09-22T00:00:00Z".to_owned(),
            window_end: "2026-10-23T00:00:00Z".to_owned(),
        },
        workflow_sources: spec().workflow_sources,
        documents: vec![
            metadata_document(11, 100, 1),
            metadata_document(12, 100, 2),
            GitHubEvidenceDocumentV1 {
                kind: GitHubEvidenceDocumentKindV1::RunArtifactsPage,
                workflow_path: Some(".github/workflows/producer.yml".to_owned()),
                run_id: Some(100),
                attempt: None,
                artifact_id: None,
                page: Some(1),
                path: "documents/artifacts-100-0001.json".to_owned(),
                size_bytes: 1,
                sha256: "3".repeat(64),
            },
        ],
        run_attempts: vec![
            attempt(1, true, Some("failure")),
            attempt(1, false, Some("failure")),
            attempt(2, false, Some("success")),
        ],
        artifacts: vec![artifact(11, 1), artifact(12, 2)],
    }
}

#[test]
fn predecessor_is_temporal_context_but_every_in_window_attempt_stays_required() {
    let request = spec();
    let mut evidence = snapshot();
    let mut paths = std::collections::BTreeSet::new();
    let mut bytes = 0;
    validate_artifacts(&evidence, &request, &mut paths, &mut bytes)
        .expect("in-window artifacts validate without predecessor archive");

    evidence.artifacts.pop();
    let mut paths = std::collections::BTreeSet::new();
    assert!(validate_artifacts(&evidence, &request, &mut paths, &mut 0).is_err());

    let mut evidence = snapshot();
    evidence
        .run_attempts
        .retain(|attempt| attempt.is_predecessor);
    let mut paths = std::collections::BTreeSet::new();
    assert!(validate_artifacts(&evidence, &request, &mut paths, &mut 0).is_err());
}

#[test]
fn predecessor_artifact_is_rejected_and_failed_first_attempt_is_not_dropped() {
    let request = spec();
    let mut evidence = snapshot();
    evidence.documents.insert(2, metadata_document(13, 99, 1));
    evidence.artifacts.push(GitHubEvidenceArtifactCaptureV1 {
        source: source(),
        artifact_id: 13,
        run_id: 99,
        name: mise_task_suite_artifact_name_for_attempt("producer", "ledger", 99, 1),
        path: "archives/13.zip".to_owned(),
        metadata_document: 2,
        size_bytes: 4,
        sha256: "4".repeat(64),
    });
    let mut paths = std::collections::BTreeSet::new();
    assert!(validate_artifacts(&evidence, &request, &mut paths, &mut 0).is_err());

    let mut evidence = snapshot();
    assert_eq!(
        evidence.run_attempts[1].conclusion.as_deref(),
        Some("failure")
    );
    evidence.artifacts.remove(0);
    let mut paths = std::collections::BTreeSet::new();
    assert!(validate_artifacts(&evidence, &request, &mut paths, &mut 0).is_err());
}
