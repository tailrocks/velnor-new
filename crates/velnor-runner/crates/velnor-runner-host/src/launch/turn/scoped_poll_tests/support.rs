use std::path::PathBuf;

use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, ConnectionJournal,
    CreatedScaleSetResponse, HeaderAllocation, Journal, ScaleSetCreateRequest,
};

pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-poll-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    pub(crate) fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) async fn session_scope(
    scratch: &Scratch,
) -> Result<(ConnectionJournal, i64, HeaderAllocation), String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    let binding = session_binding()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    let create = ScaleSetCreateRequest::new(&binding, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&create)
        .await
        .map_err(|error| error.to_string())?;
    let create_claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Scale Set create claim missing".to_owned())?;
    let created = CreatedScaleSetResponse::verified(
        &create,
        10_101,
        "integration",
        binding.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(create_claim, created)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "session create claim missing".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-scoped-poll", 0)
        .await
        .map_err(|error| error.to_string())?;
    Ok((scope, bound.intent_id(), bound.allocation().clone()))
}

fn session_binding() -> Result<RepositoryConnection, String> {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("scoped-poll-test").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-poll-test")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

pub(crate) async fn record_acquired_request(
    scope: &ConnectionJournal,
    request_id: i64,
    message_id: i64,
) -> Result<crate::journal::AssignmentIdentity, String> {
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    scope
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    if !scope
        .claim_request_acquire(&identity)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("request acquire claim missing".to_owned());
    }
    scope
        .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![request_id]))
        .await
        .map_err(|error| error.to_string())?;
    Ok(identity)
}

pub(crate) fn authorized_source() -> VerifiedWorkflowSource {
    let workflow_ref = "owner/repository/.github/workflows/build.yml@refs/heads/main".to_owned();
    VerifiedWorkflowSource {
        repository_id: 101,
        owner_name: "owner".to_owned(),
        repository_name: "repository".to_owned(),
        workflow_run_id: 10_088,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow_ref,
            owner_name: "owner".to_owned(),
            repository_name: "repository".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

pub(crate) fn repository_connection() -> Result<RepositoryConnection, String> {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("scoped-poll-test").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-poll-test")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}
