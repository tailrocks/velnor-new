//! Prepare fixed, isolated control-file publisher work.

use velnor_resource_probe::execution_publication::ExecutionPublicationKind;

use crate::{
    error::HostError,
    journal::{Journal, ProbeRow, ProbeWork},
    journal::{ProbePhase, VerifiedMetadataInitialization},
    worker::execution_authorization::{VerifiedProofChannelBinding, inspect_metadata_volume},
};

use self::projection::{
    PublicationProjection, PublicationRecoveryProjection, request as make_request,
};
use std::time::Instant;
use uuid::Uuid;

use crate::{
    journal::ExecutionPublicationParent,
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

use bollard::Docker;

mod execute;
mod projection;
mod recover;
mod request;
pub(super) use recover::recover_active;

pub(crate) struct ControlPublicationInput<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) selected: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) release_secret: &'a super::ReleaseReadSecret,
    pub(crate) metadata: &'a VerifiedMetadataInitialization,
    pub(crate) channel: &'a VerifiedProofChannelBinding,
    pub(crate) guest_boot_id: &'a str,
    pub(crate) deadline: Instant,
}

mod control;
pub(in crate::launch) use control::publish_challenge_with_provider;
pub(crate) use control::publish_decision;

fn validate_job_facts_binding(
    claim: &crate::journal::JobFactsPublicationClaim,
    ready: &crate::worker::storage_initialization::VerifiedStorageInitializationReady,
    selected: &SelectedEngineContext,
    channel: &VerifiedProofChannelBinding,
) -> Result<(), HostError> {
    if channel.matches_initialized_job_facts(ready)
        && claim.reservation_id() == ready.reservation_id()
        && claim.operation_id() == ready.operation_id()
        && claim.engine_id() == selected.engine_id()
        && claim.docker_root_digest() == selected.docker_root_digest()
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

/// Publish the claimed cache admission document in the fixed Runner mount.
pub(crate) struct JobFactsPublicationInput<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) connection_journal: &'a crate::journal::ConnectionJournal,
    pub(crate) selected: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) connection: &'a crate::config::RepositoryConnection,
    pub(crate) guest_image: &'a crate::worker::guest_image::GuestImageFile,
    pub(crate) ready: &'a crate::worker::storage_initialization::VerifiedStorageInitializationReady,
    pub(crate) claim: &'a crate::journal::JobFactsPublicationClaim,
    pub(crate) channel: &'a VerifiedProofChannelBinding,
    pub(crate) deadline: Instant,
}

pub(in crate::launch) async fn publish_job_facts_with_provider<
    P: crate::launch::resource_probe::ImageProvider,
>(
    input: JobFactsPublicationInput<'_>,
    provider: &P,
) -> Result<crate::launch::bootstrap::JobFactsPublication, HostError> {
    let journal = input.connection_journal;
    let claim = input.claim;
    match publish_job_facts_inner(input, provider).await {
        Ok(proof) => Ok(proof),
        Err(error) => {
            journal.mark_job_facts_publication_uncertain(claim).await?;
            Err(error)
        }
    }
}

struct PreparedJobFactsPublication<'a> {
    snapshot: crate::journal::BoundJobFactsSnapshot,
    guest_image_bytes: &'a [u8],
    runtime_identity: velnor_resource_probe::job_facts_wire::JobFactsRuntimeIdentityDirectory,
    binding_sha256: String,
}

async fn prepare_job_facts_publication<'a>(
    input: &'a JobFactsPublicationInput<'_>,
) -> Result<PreparedJobFactsPublication<'a>, HostError> {
    validate_job_facts_binding(input.claim, input.ready, input.selected, input.channel)?;
    let guest_image_bytes = input
        .guest_image
        .verified_document_bytes(input.connection, input.images)?;
    let guest_image_sha256 = sha256_hex(guest_image_bytes);
    let snapshot = input
        .connection_journal
        .job_facts_snapshot(input.claim, input.ready, input.channel, &guest_image_sha256)
        .await?;
    if snapshot.physical_binding_digest() != input.claim.instance_binding_digest()
        || input.channel.instance_binding_digest() != input.claim.instance_binding_digest()
    {
        return Err(HostError::Identity);
    }
    let runtime_identity = input.ready.runtime_identity();
    let runtime_identity_directory =
        velnor_resource_probe::job_facts_wire::JobFactsRuntimeIdentityDirectory {
            device_major: runtime_identity.device().0,
            device_minor: runtime_identity.device().1,
            inode: runtime_identity.inode(),
            owner_uid: runtime_identity.owner().0,
            owner_gid: runtime_identity.owner().1,
            mode: runtime_identity.mode(),
        };
    let binding_sha256 = job_facts_publication_binding(
        input.claim,
        input.ready,
        input.connection,
        guest_image_bytes,
    )?;
    Ok(PreparedJobFactsPublication {
        snapshot,
        guest_image_bytes,
        runtime_identity: runtime_identity_directory,
        binding_sha256,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 0x0f)]),
            ]
        })
        .collect()
}

async fn publish_job_facts_inner<P: crate::launch::resource_probe::ImageProvider>(
    input: JobFactsPublicationInput<'_>,
    provider: &P,
) -> Result<crate::launch::bootstrap::JobFactsPublication, HostError> {
    let prepared = prepare_job_facts_publication(&input).await?;
    let probe = super::verify_publication_probe_image_until_with_provider(
        input.docker,
        input.journal,
        input.selected,
        provider,
        input.deadline,
    )
    .await?;
    let volume = inspect_metadata_volume(
        input.docker,
        input.selected,
        input.images,
        input.ready.final_readiness(),
        input.deadline,
    )
    .await?;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = request::job_facts_request(
        &operation_id,
        &input.channel.observation().guest_boot_id,
        prepared.snapshot.bytes(),
        input.channel.job_facts_directory(),
        prepared.guest_image_bytes,
        prepared.runtime_identity.clone(),
        &prepared.binding_sha256,
    )?;
    publish_prepared_job_facts(&input, prepared, probe, volume, request).await
}

async fn publish_prepared_job_facts(
    input: &JobFactsPublicationInput<'_>,
    prepared: PreparedJobFactsPublication<'_>,
    probe: crate::launch::resource_probe::VerifiedPublicationProbeImage,
    volume: crate::worker::execution_authorization::VerifiedMetadataVolume,
    request: velnor_resource_probe::job_facts_wire::JobFactsPublicationContext,
) -> Result<crate::launch::bootstrap::JobFactsPublication, HostError> {
    let parent = ExecutionPublicationParent::job_facts(
        input.claim,
        input.ready.final_readiness(),
        &input.channel.observation().guest_boot_id,
        &request.payload_sha256,
    )?;
    let expected_request = request.clone();
    let record = execute::publish_job_facts(
        execute::PublisherExecution {
            docker: input.docker,
            journal: input.journal,
            selected: input.selected,
            images: input.images,
            probe: &probe,
            metadata: input.ready.final_readiness(),
            metadata_volume: &volume,
            deadline: input.deadline,
        },
        request,
        parent,
    )
    .await?;
    crate::launch::bootstrap::job_facts_publication(
        input.claim,
        &prepared.snapshot,
        input.ready,
        input.channel,
        &expected_request,
        &record,
    )
}

fn job_facts_publication_binding(
    claim: &crate::journal::JobFactsPublicationClaim,
    ready: &crate::worker::storage_initialization::VerifiedStorageInitializationReady,
    connection: &crate::config::RepositoryConnection,
    guest_image: &[u8],
) -> Result<String, HostError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    use sha2::{Digest as _, Sha256};
    let connection_bytes = connection.canonical_identity_bytes()?;
    let mut hasher = Sha256::new();
    for value in [
        b"velnor-job-facts-publication-binding-v1".as_slice(),
        connection_bytes.as_slice(),
        claim.instance_binding_digest().as_bytes(),
        ready.layout_digest().as_bytes(),
        claim.engine_id().as_bytes(),
        claim.docker_root_digest().as_bytes(),
        claim.runner_container_id().as_bytes(),
        claim.dind_container_id().as_bytes(),
        guest_image,
    ] {
        let length = u64::try_from(value.len()).map_err(|_| HostError::Frame)?;
        hasher.update(length.to_be_bytes());
        hasher.update(value);
    }
    let mut output = String::with_capacity(64);
    for byte in hasher.finalize() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

/// Proof that one exact publication helper was removed from its selected engine.
pub(crate) struct VerifiedExecutionPublicationCleanup {
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    operation_name: String,
    container_id: String,
    phase: ProbePhase,
}

impl VerifiedExecutionPublicationCleanup {
    fn after_exact_removal(
        projection: &PublicationProjection,
        container_id: &str,
        phase: ProbePhase,
    ) -> Result<Self, HostError> {
        if phase != ProbePhase::RemoveRequested
            || projection.name
                != format!("velnor-execution-publication-{}", projection.operation_id)
            || !valid_container_id(container_id)
        {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            operation_id: projection.operation_id.clone(),
            engine_id: projection.engine_id.clone(),
            docker_root_digest: projection.root.digest().to_owned(),
            operation_name: projection.name.clone(),
            container_id: container_id.to_owned(),
            phase,
        })
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }

    pub(crate) const fn phase(&self) -> ProbePhase {
        self.phase
    }
}

/// Build one request from the exact claimed challenge and measured Runner channel.
pub(crate) fn challenge_request(
    claim: &crate::journal::execution_authorization::ExecutionChallengePublicationClaim,
    channel: &VerifiedProofChannelBinding,
    guest_boot_id: &str,
    operation_id: String,
) -> Result<velnor_resource_probe::execution_publication::ExecutionPublicationContext, HostError> {
    make_request(
        operation_id,
        guest_boot_id,
        ExecutionPublicationKind::Challenge,
        claim.canonical_bytes(),
        claim.challenge_digest(),
        channel.control_directory(),
    )
}

/// Build one request from the exact claimed decision and measured Runner channel.
pub(crate) fn decision_request(
    claim: &crate::journal::execution_authorization::ExecutionDecisionDeliveryClaim,
    channel: &VerifiedProofChannelBinding,
    guest_boot_id: &str,
    operation_id: String,
) -> Result<velnor_resource_probe::execution_publication::ExecutionPublicationContext, HostError> {
    make_request(
        operation_id,
        guest_boot_id,
        ExecutionPublicationKind::Decision,
        claim.canonical_bytes(),
        claim.decision_digest(),
        channel.control_directory(),
    )
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "publication/recovery_tests.rs"]
mod recovery_tests;
