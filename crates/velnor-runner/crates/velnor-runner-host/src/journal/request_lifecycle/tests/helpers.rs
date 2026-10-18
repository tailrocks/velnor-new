use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::scale_set_create::{CreatedScaleSetResponse, ScaleSetCreateRequest};
use crate::journal::{Journal, SourceAuthorizationResult, fixture_profile};
use crate::launch::steps::source::{AuthorizedSourceReceipt, test_receipt};
use velnor_runner_github::SourceMetadata;
use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-request-journal-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    pub(super) fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn binding(id: &str, repository_id: i64) -> Result<RepositoryConnection, HostError> {
    let labels = vec!["velnor".to_owned(), "integration".to_owned()];
    RepositoryConnection::try_new(
        ConnectionId::parse(id)?,
        repository_id,
        format!("owner-{repository_id}/repo-{repository_id}"),
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

pub(super) async fn created_scope(
    journal: &Journal,
    binding: &RepositoryConnection,
    set_id: i64,
) -> Result<ConnectionJournal, HostError> {
    journal.configure_storage_profile(fixture_profile()).await?;
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
        set_id,
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

pub(super) async fn receipt(
    scope: &ConnectionJournal,
    request_id: i64,
    message_id: i64,
    repository_id: i64,
) -> Result<AuthorizedSourceReceipt, HostError> {
    let binding = scope.source_authorization_binding().await?;
    Ok(test_receipt(
        &binding,
        request_id,
        message_id,
        source(repository_id, request_id + 1000),
        SourcePolicyMode::Repository,
    ))
}

pub(super) fn source(repository_id: i64, run_id: i64) -> VerifiedWorkflowSource {
    let workflow_ref = format!(
        "owner-{repository_id}/repo-{repository_id}/.github/workflows/build.yml@refs/heads/main"
    );
    VerifiedWorkflowSource {
        repository_id,
        owner_name: format!("owner-{repository_id}"),
        repository_name: format!("repo-{repository_id}"),
        workflow_run_id: run_id,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow_ref,
            owner_name: format!("owner-{repository_id}"),
            repository_name: format!("repo-{repository_id}"),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

pub(super) fn event_source(repository_id: i64, run_id: i64) -> SourceMetadata {
    SourceMetadata {
        owner_name: Some(format!("owner-{repository_id}")),
        repository_name: Some(format!("repo-{repository_id}")),
        job_workflow_ref: Some(format!(
            "owner-{repository_id}/repo-{repository_id}/.github/workflows/build.yml@refs/heads/main"
        )),
        workflow_run_id: Some(run_id),
        event_name: Some("push".to_owned()),
    }
}

pub(super) fn runner_name(nonce: char) -> String {
    format!("v{}", nonce.to_string().repeat(32))
}

pub(super) async fn record_acquired(
    scope: &ConnectionJournal,
    request_id: i64,
) -> Result<(), HostError> {
    let identity = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source = receipt(scope, request_id, request_id + 100, 101).await?;
    if scope
        .record_source_authorization(&identity, &source)
        .await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    if !scope.claim_request_acquire(&identity).await? {
        return Err(HostError::Journal);
    }
    scope
        .record_request_acquire(
            &identity,
            crate::journal::AssignedAcquireResult::Returned(vec![request_id]),
        )
        .await
}
