use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::request_lifecycle::tests::helpers::{
    JobFactsReaderFixture, Scratch, binding, created_scope,
    provision_acquired_request_with_initialization,
};
use crate::journal::{CapacityLimits, JobFactsPublicationClaim, Journal};
use crate::worker::execution_authorization::{
    VerifiedProofChannelBinding, test_channel_for_ready_job_facts,
};
use velnor_runner_github::RunnerReference;

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
    let ready = provision_acquired_request_with_initialization(&scope, 70, nonce, nonce).await?;
    let runner = RunnerReference {
        id: 770,
        name: format!("v{}", nonce.to_string().repeat(32)),
        runner_scale_set_id: 501,
    };
    let (generation, digest) = scope.test_active_authority_identity().await?;
    let jit = scope
        .claim_runner_jit_for_request(
            ready.intent_id,
            ready.reservation_id,
            70,
            &runner.name,
            generation,
            &digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.bind_jit_runner(jit, &runner).await?;
    let channel = test_channel_for_ready_job_facts(
        crate::worker::execution_authorization::TestProofChannelIdentity {
            reservation_id: ready.reservation_id,
            operation_id: ready.intent_id,
            connection_id: scope.connection_id().as_str().to_owned(),
            scale_set_id: 501,
            witness_request_id: 70,
            engine_id: crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID.to_owned(),
            docker_root_digest: ready.docker_root_digest.clone(),
            runner_id: runner.id,
            runner_name: runner.name,
            runner_container_id: ready.runner_container_id.clone(),
            dind_container_id: ready.dind_container_id.clone(),
            instance_binding_digest: ready.instance_binding_digest.clone(),
        },
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
