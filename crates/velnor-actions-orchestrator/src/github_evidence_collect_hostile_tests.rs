use super::hostile_support::{HostileReader, Mutation};
use super::support::collect;

#[test]
fn rejects_well_formed_foreign_repository_workflow_run_and_attempt_identities() {
    for mutation in [
        Mutation::RepositoryId,
        Mutation::RepositoryName,
        Mutation::WorkflowPath,
        Mutation::RunWorkflowId,
        Mutation::RunEvent,
        Mutation::RunBranch,
        Mutation::RunHead,
        Mutation::AttemptRunId,
        Mutation::AttemptNumber,
        Mutation::AttemptEvent,
        Mutation::AttemptBranch,
        Mutation::AttemptHead,
    ] {
        assert_rejected_before_index(mutation);
    }
}

#[test]
fn rejects_duplicate_ids_at_workflow_run_and_artifact_inventory_boundaries() {
    for mutation in [
        Mutation::DuplicateWorkflowId,
        Mutation::DuplicateRunId,
        Mutation::DuplicateArtifactId,
    ] {
        assert_rejected_before_index(mutation);
    }
}

#[test]
fn rejects_selected_artifact_identity_and_download_receipt_mismatches() {
    for mutation in [
        Mutation::ArtifactExpired,
        Mutation::ArtifactEmpty,
        Mutation::ArtifactWrongSize,
        Mutation::ArtifactWrongDigest,
        Mutation::ArtifactByIdChanged,
        Mutation::ArchiveReceiptMismatch,
    ] {
        assert_rejected_before_index(mutation);
    }
}

#[test]
fn workflow_run_and_artifact_page_counts_must_remain_stable() {
    for mutation in [
        Mutation::WorkflowRunsCountDrift,
        Mutation::WorkflowRunsTruncatedSecondPage,
        Mutation::ArtifactCountDrift,
        Mutation::ArtifactTruncatedSecondPage,
    ] {
        let reader = HostileReader::new(mutation);
        let temp = tempfile::tempdir().expect("temporary fixture root");
        let error = collect(&reader, &temp).expect_err("page mutation must be rejected");
        let expected_error = match mutation {
            Mutation::WorkflowRunsCountDrift | Mutation::WorkflowRunsTruncatedSecondPage => {
                "internal: github_evidence_run_pages_incomplete"
            }
            Mutation::ArtifactCountDrift | Mutation::ArtifactTruncatedSecondPage => {
                "internal: github_evidence_artifact_pages_incomplete"
            }
            _ => unreachable!("only page mutations appear in this test"),
        };
        assert_eq!(error.to_string(), expected_error);
        assert!(
            reader.saw_second_page(),
            "reader reached changed second page"
        );
        assert_no_snapshot_index(&temp);
    }
}

#[test]
fn collector_decodes_malformed_and_duplicate_key_raw_response_bytes() {
    for mutation in [
        Mutation::MalformedRepositoryBytes,
        Mutation::DuplicateRepositoryKeyBytes,
    ] {
        assert_rejected_before_index(mutation);
    }
}

fn assert_rejected_before_index(mutation: Mutation) {
    let reader = HostileReader::new(mutation);
    let temp = tempfile::tempdir().expect("temporary fixture root");
    assert!(collect(&reader, &temp).is_err());
    assert_no_snapshot_index(&temp);
}

fn assert_no_snapshot_index(temp: &tempfile::TempDir) {
    assert!(
        !temp
            .path()
            .join("velnor/github-evidence-bundle/index.json")
            .exists(),
        "rejected capture must not publish a snapshot index"
    );
}
