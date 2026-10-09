use super::{ActionsAttemptAdapterError, ActionsAttemptAdapterOutcome, adapt_actions_attempt_read};
use velnor_actions_orchestrator_merge_ports::{
    ActionsAttemptArtifactView, ActionsAttemptJobView, CompleteActionsAttemptView,
    ScopedCompareRequest,
};
use velnor_runner_github::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptJobEvidence,
    ActionsWorkflowAttemptProviderEvidence, ActionsWorkflowAttemptProviderRead,
    ActionsWorkflowRunArtifactEvidence,
};

const REPOSITORY: &str = "ChainArgos/java-monorepo";
const REPOSITORY_ID: i64 = 12_345;
const RUN_ID: i64 = 424_242;
const ATTEMPT: u32 = 2;
const HEAD: &str = "abababababababababababababababababababab";

fn evidence() -> ActionsWorkflowAttemptProviderEvidence {
    ActionsWorkflowAttemptProviderEvidence {
        repository_id: REPOSITORY_ID,
        repository_full_name: REPOSITORY.to_owned(),
        workflow_run_id: RUN_ID,
        attempt: ATTEMPT,
        head_sha: HEAD.to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        event: "pull_request".to_owned(),
        head_branch: Some("main".to_owned()),
        head_repository_id: Some(REPOSITORY_ID),
        head_repository_full_name: Some(REPOSITORY.to_owned()),
        status: "completed".to_owned(),
        conclusion: Some("success".to_owned()),
        jobs: vec![ActionsWorkflowAttemptJobEvidence {
            id: 10_601,
            check_run_id: Some(601),
            run_id: RUN_ID,
            name: "not-a-logical-job-key".to_owned(),
            head_sha: HEAD.to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
            runner_id: Some(77),
            runner_name: Some("hosted-runner".to_owned()),
            runner_group_id: Some(3),
            runner_group_name: Some("Default".to_owned()),
            workflow_name: Some("CI".to_owned()),
            head_branch: Some("main".to_owned()),
            labels: Some(vec!["ubuntu-26.04".to_owned()]),
        }],
        artifacts: vec![ActionsWorkflowRunArtifactEvidence {
            id: 501,
            name: "task-report-hosted".to_owned(),
            size_in_bytes: 256,
            expired: false,
            digest: Some("sha256:0123456789abcdef".to_owned()),
            workflow_run_id: RUN_ID,
            repository_id: REPOSITORY_ID,
            head_repository_id: Some(REPOSITORY_ID),
            head_branch: Some("main".to_owned()),
            head_sha: HEAD.to_owned(),
        }],
    }
}

fn request(repository: &str, run_id: i64, attempt: u32) -> ScopedCompareRequest<'_> {
    ScopedCompareRequest {
        repository,
        run_id,
        attempt,
    }
}

fn complete(
    evidence: ActionsWorkflowAttemptProviderEvidence,
) -> ActionsWorkflowAttemptProviderRead {
    ActionsWorkflowAttemptProviderRead::Complete(Box::new(evidence))
}

#[test]
fn complete_provider_rows_map_exact_rest_job_checkrun_and_artifact_ids() {
    let adapted = adapt_actions_attempt_read(
        complete(evidence()),
        request(REPOSITORY, RUN_ID, ATTEMPT),
        HEAD,
    )
    .expect("complete inventory adapts");
    let ActionsAttemptAdapterOutcome::Complete(view) = adapted else {
        panic!("complete provider result became unavailable");
    };
    assert_eq!(view.repository_id(), REPOSITORY_ID);
    assert_eq!(view.repository_full_name(), REPOSITORY);
    assert_eq!(view.workflow_run_id(), RUN_ID);
    assert_eq!(view.attempt(), ATTEMPT);
    assert_eq!(view.head_sha(), HEAD);
    assert_eq!(view.run_status(), "completed");
    assert_eq!(view.run_conclusion(), Some("success"));
    assert_eq!(view.jobs().len(), 1);
    assert_eq!(view.jobs()[0].actions_job_id(), 10_601);
    assert_eq!(view.jobs()[0].check_run_id(), Some(601));
    assert_ne!(view.jobs()[0].actions_job_id(), 601);
    assert_eq!(view.jobs()[0].workflow_run_id(), RUN_ID);
    assert_eq!(view.jobs()[0].head_sha(), HEAD);
    assert_eq!(view.jobs()[0].status(), "completed");
    assert_eq!(view.jobs()[0].conclusion(), Some("success"));
    assert_eq!(view.artifacts().len(), 1);
    assert_eq!(view.artifacts()[0].artifact_id(), 501);
    assert_eq!(view.artifacts()[0].artifact_name(), "task-report-hosted");
    assert!(!view.artifacts()[0].expired());
    assert_eq!(view.artifacts()[0].workflow_run_id(), RUN_ID);
    assert_eq!(view.artifacts()[0].repository_id(), REPOSITORY_ID);
    assert_eq!(view.artifacts()[0].head_sha(), HEAD);
}

#[test]
fn missing_or_inconsistent_page_outcome_never_exposes_partial_rows() {
    let outcome = adapt_actions_attempt_read(
        ActionsWorkflowAttemptProviderRead::Unavailable(
            ActionsWorkflowAttemptEvidenceGap::InconsistentJobPages,
        ),
        request(REPOSITORY, RUN_ID, ATTEMPT),
        HEAD,
    )
    .expect("unavailable read is a typed gap");
    assert_eq!(
        outcome,
        ActionsAttemptAdapterOutcome::Unavailable(
            ActionsWorkflowAttemptEvidenceGap::InconsistentJobPages
        )
    );
}

#[test]
fn repository_run_attempt_and_head_must_match_the_requested_scope() {
    for (requested, head) in [
        (request("Other/java-monorepo", RUN_ID, ATTEMPT), HEAD),
        (request(REPOSITORY, RUN_ID + 1, ATTEMPT), HEAD),
        (request(REPOSITORY, RUN_ID, ATTEMPT + 1), HEAD),
        (
            request(REPOSITORY, RUN_ID, ATTEMPT),
            "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        ),
    ] {
        assert_eq!(
            adapt_actions_attempt_read(complete(evidence()), requested, head),
            Err(ActionsAttemptAdapterError::ProviderScopeMismatch),
        );
    }
}

#[test]
fn duplicate_artifact_ids_are_rejected_but_names_are_not_used_as_identity() {
    let mut duplicate_id = evidence();
    let mut second = duplicate_id.artifacts[0].clone();
    second.name = "different-name".to_owned();
    duplicate_id.artifacts.push(second);
    assert_eq!(
        adapt_actions_attempt_read(
            complete(duplicate_id),
            request(REPOSITORY, RUN_ID, ATTEMPT),
            HEAD,
        ),
        Err(ActionsAttemptAdapterError::InvalidProviderInventory),
    );

    let mut same_name = evidence();
    let mut second = same_name.artifacts[0].clone();
    second.id = 502;
    same_name.artifacts.push(second);
    let outcome = adapt_actions_attempt_read(
        complete(same_name),
        request(REPOSITORY, RUN_ID, ATTEMPT),
        HEAD,
    )
    .expect("distinct immutable artifact IDs are selectable by producer output");
    assert!(matches!(outcome, ActionsAttemptAdapterOutcome::Complete(_)));
}

#[test]
fn duplicate_or_foreign_checkrun_and_job_rows_are_rejected() {
    let mut duplicate_job = evidence();
    duplicate_job.jobs.push(duplicate_job.jobs[0].clone());
    assert_eq!(
        adapt_actions_attempt_read(
            complete(duplicate_job),
            request(REPOSITORY, RUN_ID, ATTEMPT),
            HEAD,
        ),
        Err(ActionsAttemptAdapterError::InvalidProviderInventory),
    );

    let mut foreign_artifact = evidence();
    foreign_artifact.artifacts[0].repository_id += 1;
    assert_eq!(
        adapt_actions_attempt_read(
            complete(foreign_artifact),
            request(REPOSITORY, RUN_ID, ATTEMPT),
            HEAD,
        ),
        Err(ActionsAttemptAdapterError::InvalidProviderInventory),
    );
}
