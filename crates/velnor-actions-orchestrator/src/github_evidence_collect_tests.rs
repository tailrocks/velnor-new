use serde_json::{Value, json};

#[path = "github_evidence_collect_admission_tests.rs"]
mod admission_tests;
#[path = "github_evidence_collect_hostile_support.rs"]
mod hostile_support;
#[path = "github_evidence_collect_hostile_tests.rs"]
mod hostile_tests;
#[path = "github_evidence_import_context_hostile_tests.rs"]
mod import_context_hostile_tests;
#[path = "github_evidence_collect_test_support.rs"]
mod support;
use support::{FixtureReader, FixtureScenario, collect, spec};

const RUN_TIME: &str = "2026-10-22T12:00:00Z";
const PREDECESSOR_TIME: &str = "2026-09-21T12:00:00Z";

fn run_identity(run_id: u64) -> (u64, &'static str) {
    match run_id {
        100 => (10, RUN_TIME),
        101 => (10, PREDECESSOR_TIME),
        200 => (20, RUN_TIME),
        201 => (20, PREDECESSOR_TIME),
        _ => (0, RUN_TIME),
    }
}

fn run_summary(
    id: u64,
    workflow_id: u64,
    run_attempt: u32,
    created_at: &str,
    conclusion: &str,
) -> Value {
    json!({
        "id": id,
        "workflow_id": workflow_id,
        "run_attempt": run_attempt,
        "event": "push",
        "head_branch": "main",
        "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "status": "completed",
        "conclusion": conclusion,
        "created_at": created_at,
        "repository": { "id": 1, "full_name": "tailrocks/velnor" },
        "head_repository": { "id": 1, "full_name": "tailrocks/velnor" }
    })
}
#[test]
fn captures_primary_ci_metadata_and_every_producer_attempt_with_original_archives() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let snapshot = collect(&FixtureReader::default(), &temp).expect("complete fixture capture");
    snapshot
        .validate_for_spec(&spec())
        .expect("captured closure validates");
    assert_eq!(
        snapshot
            .run_attempts
            .iter()
            .filter(|row| !row.is_predecessor)
            .count(),
        4
    );
    assert_eq!(snapshot.artifacts.len(), 2);
    assert!(snapshot.artifacts.iter().all(|row| row.run_id == 200));
    assert!(snapshot.run_attempts.iter().any(|row| row.run_id == 200
        && row.attempt == 1
        && row.conclusion.as_deref() == Some("failure")));
    assert!(snapshot.run_attempts.iter().any(|row| row.run_id == 100
        && row.attempt == 1
        && row.conclusion.as_deref() == Some("startup_failure")));
    assert!(snapshot.run_attempts.iter().any(|row| row.run_id == 100
        && row.attempt == 2
        && row.conclusion.as_deref() == Some("success")));
    assert!(
        snapshot
            .run_attempts
            .iter()
            .filter(|row| row.is_predecessor)
            .all(|row| row.created_at < snapshot.acquisition.window_start)
    );
    let imported = super::super::import::verify_test_bundle(
        &temp.path().join("velnor/github-evidence-bundle"),
        &spec(),
    )
    .expect("credential-free import accepts retained startup failure");
    assert_eq!(imported, snapshot);
    assert!(imported.run_attempts.iter().any(|row| {
        row.run_id == 100
            && row.attempt == 1
            && row.conclusion.as_deref() == Some("startup_failure")
    }));
}

#[test]
fn rejects_jobs_without_exact_returned_run_and_event_commit_lineage() {
    for scenario in [
        FixtureScenario::WrongJobRunId,
        FixtureScenario::WrongJobHeadSha,
        FixtureScenario::MissingJobRunId,
        FixtureScenario::MissingJobHeadSha,
        FixtureScenario::DuplicateJobId,
        FixtureScenario::WrongTaskJobName,
    ] {
        let temp = tempfile::tempdir().expect("temporary fixture root");
        assert!(collect(&FixtureReader::for_scenario(scenario), &temp).is_err());
    }
}

#[test]
fn job_pages_require_the_complete_declared_count_and_accept_complete_pagination() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let snapshot = collect(
        &FixtureReader::for_scenario(FixtureScenario::MultiPageJobs),
        &temp,
    )
    .expect("complete two-page job inventory");
    assert!(snapshot.documents.iter().any(|document| {
        document.kind == velnor_actions_contract::GitHubEvidenceDocumentKindV1::RunAttemptJobsPage
            && document.page == Some(2)
    }));

    let temp = tempfile::tempdir().expect("temporary fixture root");
    assert!(
        collect(
            &FixtureReader::for_scenario(FixtureScenario::TruncatedJobsSecondPage),
            &temp,
        )
        .is_err()
    );
}

#[test]
fn rejects_semantically_invalid_archive_when_receipt_and_indexed_identity_match() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let reader = FixtureReader::for_scenario(FixtureScenario::InvalidArchive);
    assert!(collect(&reader, &temp).is_err());
}

#[test]
fn rejects_unlisted_extra_attempt_artifacts_for_a_declared_producer() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let reader = FixtureReader::for_scenario(FixtureScenario::ExtraAttemptArtifact);
    assert!(collect(&reader, &temp).is_err());
}

#[test]
fn rejects_conclusions_outside_the_shared_closed_vocabulary() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let reader = FixtureReader::for_scenario(FixtureScenario::BadConclusion);
    assert!(collect(&reader, &temp).is_err());
}

#[test]
fn rejects_missing_predecessor_incomplete_workflow_page_and_missing_failed_attempt_archive() {
    for reader in [
        FixtureReader::for_scenario(FixtureScenario::OmitPredecessor),
        FixtureReader::for_scenario(FixtureScenario::PartialWorkflowPage),
        FixtureReader::omitting_attempt_artifact(1),
        FixtureReader::for_scenario(FixtureScenario::OmitProducerRun),
    ] {
        let temp = tempfile::tempdir().expect("temporary fixture root");
        assert!(collect(&reader, &temp).is_err());
    }
}
