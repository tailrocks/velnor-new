//! Source receipt and closed-world acquire regressions.

mod admission;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::scale_set_create::{CreatedScaleSetResponse, ScaleSetCreateRequest};
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal, ReservationRequest,
    ReservationResult, SourceAuthorizationResult,
};
use crate::launch::StartPermit;
use crate::launch::steps::source::test_receipt;
use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-source-journal-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn binding(
    id: &str,
    repository_id: i64,
    repository: &str,
) -> Result<RepositoryConnection, HostError> {
    let labels = vec!["velnor".to_owned(), "integration".to_owned()];
    RepositoryConnection::try_new(
        ConnectionId::parse(id)?,
        repository_id,
        repository.to_owned(),
        KeychainReference::parse(&format!("keychain:velnor/{id}"))?,
        ScaleSetSelector::try_new("integration", &labels)?,
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )
}

async fn created_scope(
    journal: &Journal,
    binding: &RepositoryConnection,
    scale_set_id: i64,
) -> Result<ConnectionJournal, HostError> {
    journal.bind_engine("test-engine").await?;
    let scope = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 7)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let response = CreatedScaleSetResponse::verified(
        &request,
        scale_set_id,
        "integration",
        binding.scale_set.labels(),
        true,
    )?;
    scope.record_scale_set_created(claim, response).await?;
    scope
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    Ok(scope)
}

async fn reserve_assigned(
    scope: &ConnectionJournal,
    request_id: i64,
) -> Result<(crate::journal::AssignmentIdentity, i64), HostError> {
    let assignment = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let result = scope
        .try_reserve(
            ReservationRequest {
                assignment: &assignment,
                engine_id: "test-engine",
                docker_root_digest: &root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &generation,
            },
            permit,
        )
        .await?;
    if !matches!(result, ReservationResult::Reserved { .. }) {
        return Err(HostError::Journal);
    }
    let (intent_id, fresh) = scope
        .begin_assigned_launch(&assignment, &format!("v{request_id}"))
        .await?;
    if !fresh {
        return Err(HostError::Journal);
    }
    Ok((assignment, intent_id))
}

fn source(repository_id: i64, run_id: i64) -> VerifiedWorkflowSource {
    let full_ref = "donbeave/essential-mac/.github/workflows/build.yml@refs/heads/main";
    VerifiedWorkflowSource {
        repository_id,
        owner_name: "donbeave".to_owned(),
        repository_name: "essential-mac".to_owned(),
        workflow_run_id: run_id,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: full_ref.to_owned(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: full_ref.to_owned(),
            owner_name: "donbeave".to_owned(),
            repository_name: "essential-mac".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

#[tokio::test]
async fn exact_receipts_are_scoped_replayed_and_durable() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 8_000,
            memory_bytes: 17_179_869_184,
        })
        .await?;
    let first_binding = binding("essential", 101, "donbeave/essential-mac")?;
    let second_binding = binding("other", 202, "donbeave/essential-mac")?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;
    let (first_assignment, _) = reserve_assigned(&first, 33).await?;
    let (second_assignment, _) = reserve_assigned(&second, 33).await?;
    let first_receipt = test_receipt(33, 77, source(101, 900), SourcePolicyMode::Repository);
    let second_receipt = test_receipt(33, 77, source(202, 900), SourcePolicyMode::Repository);

    assert_eq!(
        first
            .record_source_authorization(&first_assignment, &first_receipt)
            .await?,
        SourceAuthorizationResult::Recorded
    );
    assert_eq!(
        second
            .record_source_authorization(&second_assignment, &second_receipt)
            .await?,
        SourceAuthorizationResult::Recorded
    );
    assert_eq!(
        first
            .record_source_authorization(&first_assignment, &first_receipt)
            .await?,
        SourceAuthorizationResult::Replayed
    );
    let changed_receipt = test_receipt(33, 78, source(101, 900), SourcePolicyMode::Repository);
    assert_eq!(
        first
            .record_source_authorization(&first_assignment, &changed_receipt)
            .await,
        Err(HostError::Identity)
    );
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = journal.connection_scope(&first_binding).await?;
    let second = journal.connection_scope(&second_binding).await?;
    let first_assignment = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    let second_assignment = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    assert_eq!(
        first
            .record_source_authorization(&first_assignment, &first_receipt)
            .await?,
        SourceAuthorizationResult::Replayed
    );
    assert_eq!(
        second
            .record_source_authorization(&second_assignment, &second_receipt)
            .await?,
        SourceAuthorizationResult::Replayed
    );
    Ok(())
}
