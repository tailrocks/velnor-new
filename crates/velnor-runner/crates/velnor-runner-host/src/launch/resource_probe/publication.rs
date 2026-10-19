//! Prepare fixed, isolated control-file publisher work.

use velnor_resource_probe::execution_publication::ExecutionPublicationKind;

use crate::{
    error::HostError,
    journal::{Journal, ProbeRow, ProbeWork},
    journal::{
        ProbePhase, VerifiedMetadataInitialization,
        execution_authorization::{
            ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim,
        },
    },
    worker::execution_authorization::{
        ExecutionChallengePublicationReceipt, ExecutionDecisionDeliveryReceipt,
        VerifiedProofChannelBinding, challenge_publication_receipt, decision_delivery_receipt,
        inspect_metadata_volume,
    },
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

/// Publish the claimed challenge in the measured control directory.
pub(crate) async fn publish_challenge(
    input: ControlPublicationInput<'_>,
    claim: &ExecutionChallengePublicationClaim,
) -> Result<ExecutionChallengePublicationReceipt, HostError> {
    let ControlPublicationInput {
        docker,
        journal,
        selected,
        images,
        release_secret,
        metadata,
        channel,
        guest_boot_id,
        deadline,
    } = input;
    let probe = Box::pin(super::verify_publication_probe_image_until(
        docker,
        journal,
        selected,
        release_secret,
        deadline,
    ))
    .await?;
    let volume = Box::pin(inspect_metadata_volume(
        docker, selected, images, metadata, deadline,
    ))
    .await?;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = challenge_request(claim, channel, guest_boot_id, operation_id.clone())?;
    let parent = ExecutionPublicationParent::challenge(claim, metadata, guest_boot_id)?;
    let record = Box::pin(execute::publish(
        execute::PublisherExecution {
            docker,
            journal,
            selected,
            images,
            probe: &probe,
            metadata,
            metadata_volume: &volume,
            deadline,
        },
        request,
        parent,
    ))
    .await?;
    challenge_publication_receipt(
        claim,
        channel,
        metadata,
        record,
        &operation_id,
        guest_boot_id,
        &volume,
    )
}

/// Publish the claimed decision in the measured control directory.
pub(crate) async fn publish_decision(
    input: ControlPublicationInput<'_>,
    claim: &ExecutionDecisionDeliveryClaim,
) -> Result<ExecutionDecisionDeliveryReceipt, HostError> {
    let ControlPublicationInput {
        docker,
        journal,
        selected,
        images,
        release_secret,
        metadata,
        channel,
        guest_boot_id,
        deadline,
    } = input;
    let probe = Box::pin(super::verify_publication_probe_image_until(
        docker,
        journal,
        selected,
        release_secret,
        deadline,
    ))
    .await?;
    let volume = Box::pin(inspect_metadata_volume(
        docker, selected, images, metadata, deadline,
    ))
    .await?;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = decision_request(claim, channel, guest_boot_id, operation_id.clone())?;
    let parent = ExecutionPublicationParent::decision(claim, metadata, guest_boot_id)?;
    let record = Box::pin(execute::publish(
        execute::PublisherExecution {
            docker,
            journal,
            selected,
            images,
            probe: &probe,
            metadata,
            metadata_volume: &volume,
            deadline,
        },
        request,
        parent,
    ))
    .await?;
    decision_delivery_receipt(
        claim,
        channel,
        metadata,
        record,
        &operation_id,
        guest_boot_id,
        &volume,
    )
}

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
    pub(crate) release_secret: &'a super::ReleaseReadSecret,
    pub(crate) ready: &'a crate::worker::storage_initialization::VerifiedStorageInitializationReady,
    pub(crate) claim: &'a crate::journal::JobFactsPublicationClaim,
    pub(crate) channel: &'a VerifiedProofChannelBinding,
    pub(crate) deadline: Instant,
}

pub(crate) async fn publish_job_facts(
    input: JobFactsPublicationInput<'_>,
) -> Result<crate::launch::bootstrap::JobFactsPublication, HostError> {
    let JobFactsPublicationInput {
        docker,
        journal,
        connection_journal,
        selected,
        images,
        release_secret,
        ready,
        claim,
        channel,
        deadline,
    } = input;
    let publication = Box::pin(async {
        validate_job_facts_binding(claim, ready, selected, channel)?;
        let snapshot = connection_journal
            .job_facts_snapshot(claim, ready, channel)
            .await?;
        if snapshot.physical_binding_digest() != claim.instance_binding_digest()
            || channel.instance_binding_digest() != claim.instance_binding_digest()
        {
            return Err(HostError::Identity);
        }
        let probe = super::verify_publication_probe_image_until(
            docker,
            journal,
            selected,
            release_secret,
            deadline,
        )
        .await?;
        let volume =
            inspect_metadata_volume(docker, selected, images, ready.final_readiness(), deadline)
                .await?;
        let operation_id = Uuid::new_v4().simple().to_string();
        let request = request::job_facts_request(
            &operation_id,
            &channel.observation().guest_boot_id,
            snapshot.bytes(),
            channel.job_facts_directory(),
        )?;
        let parent = ExecutionPublicationParent::job_facts(
            claim,
            ready.final_readiness(),
            &channel.observation().guest_boot_id,
            &request.payload_sha256,
        )?;
        let record = execute::publish_job_facts(
            execute::PublisherExecution {
                docker,
                journal,
                selected,
                images,
                probe: &probe,
                metadata: ready.final_readiness(),
                metadata_volume: &volume,
                deadline,
            },
            request,
            parent,
        )
        .await?;
        crate::launch::bootstrap::job_facts_publication(claim, &snapshot, ready, channel, record)
    })
    .await;
    match publication {
        Ok(proof) => Ok(proof),
        Err(error) => {
            connection_journal
                .mark_job_facts_publication_uncertain(claim)
                .await?;
            Err(error)
        }
    }
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
