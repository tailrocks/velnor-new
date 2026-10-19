use crate::config::SourcePolicyMode;
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::request_lifecycle::tests::helpers::{
    JobFactsReaderFixture, Scratch, binding, created_scope,
    provision_physical_intent_with_initialization,
};
use crate::journal::{
    AssignedAcquireResult, CapacityLimits, JobFactsPublicationClaim, Journal,
    SourceAuthorizationResult,
};
use crate::launch::steps::source::test_receipt;
use crate::worker::execution_authorization::{
    VerifiedProofChannelBinding, test_channel_for_ready_job_facts,
};
use velnor_runner_github::RunnerReference;
use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

pub(super) struct BoundFixture {
    pub(super) scratch: Scratch,
    pub(super) repository: crate::config::RepositoryConnection,
    pub(super) journal: Journal,
    pub(super) scope: ConnectionJournal,
    pub(super) ready: JobFactsReaderFixture,
    pub(super) channel: VerifiedProofChannelBinding,
    pub(super) claim: JobFactsPublicationClaim,
}

pub(super) async fn bound_fixture(nonce: char) -> Result<BoundFixture, HostError> {
    // Provisioning helpers repeat one character to derive their 128-bit
    // identity. Normalize any test seed to a lowercase hexadecimal digit first.
    let nonce = char::from_digit(u32::from(nonce) & 0x0f, 16).ok_or(HostError::Identity)?;
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("job-facts-reader", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    record_request_acquired(&scope).await?;
    let ready = provision_physical_intent_with_initialization(&scope, nonce, nonce).await?;
    let runner = RunnerReference {
        id: 770,
        name: format!("v{}", nonce.to_string().repeat(32)),
        runner_scale_set_id: 501,
    };
    let jit = scope
        .claim_runner_jit(ready.intent_id, &runner.name)
        .await?
        .ok_or(HostError::Journal)?;
    scope.bind_jit_runner(jit, &runner).await?;
    let channel = test_channel_for_ready_job_facts(
        ready.reservation_id,
        ready.intent_id,
        scope.connection_id().as_str().to_owned(),
        501,
        70,
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID.to_owned(),
        ready.docker_root_digest.clone(),
        runner.id,
        runner.name,
        ready.runner_container_id.clone(),
        ready.dind_container_id.clone(),
        ready.instance_binding_digest.clone(),
        &ready.ready,
    )?;
    let claim = scope
        .claim_job_facts_publication(ready.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Ok(BoundFixture {
        scratch,
        repository,
        journal,
        scope,
        ready,
        channel,
        claim,
    })
}

async fn record_request_acquired(scope: &ConnectionJournal) -> Result<(), HostError> {
    let request_id = 70;
    let identity = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source_binding = scope.source_authorization_binding().await?;
    let workflow = "owner-101/repo-101/.github/workflows/build.yml@refs/heads/main";
    let source = VerifiedWorkflowSource {
        repository_id: 101,
        owner_name: "owner-101".to_owned(),
        repository_name: "repo-101".to_owned(),
        workflow_run_id: 170,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow.to_owned(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow.to_owned(),
            owner_name: "owner-101".to_owned(),
            repository_name: "repo-101".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    };
    let receipt = test_receipt(
        &source_binding,
        request_id,
        request_id + 100,
        source,
        SourcePolicyMode::Repository,
    );
    if scope
        .record_source_authorization(&identity, &receipt)
        .await?
        != SourceAuthorizationResult::Recorded
        || !scope.claim_request_acquire(&identity).await?
    {
        return Err(HostError::Journal);
    }
    scope
        .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![request_id]))
        .await
}
