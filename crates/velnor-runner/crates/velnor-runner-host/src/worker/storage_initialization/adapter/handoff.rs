use std::time::Duration;

use bollard::Docker;
use tokio::time::Instant;

use crate::{
    error::HostError, journal::ConnectionJournal, launch::bootstrap::VerifiedBootstrapMountHandoff,
};

use super::{remove, same_start_claim};
use crate::worker::storage_initialization::{
    PendingStorageInitialization, VerifiedStorageInitializationReady,
    pending::InitializerReleasePhase, process,
};

const HANDOFF_LIMIT: Duration = Duration::from_secs(30);

/// Persist the exact pair mounts, then release and remove the initializer helper.
pub(crate) async fn finish_after_pair_mounts(
    docker: &Docker,
    journal: &ConnectionJournal,
    pending_slot: &mut Option<PendingStorageInitialization>,
    pair: &VerifiedBootstrapMountHandoff,
) -> Result<VerifiedStorageInitializationReady, HostError> {
    let pending = pending_slot.as_mut().ok_or(HostError::Identity)?;
    let claim = &pending.claim;
    let started = &pending.started;
    let awaiting_handoff = pending.awaiting_handoff();
    let metadata_handoff = pending.metadata_handoff();
    if !same_start_claim(claim, started)
        || pair.connection_id() != awaiting_handoff.connection_id()
        || pair.connection_id() != journal.connection_id().as_str()
        || pair.reservation_id() != awaiting_handoff.reservation_id()
        || pair.operation_id() != awaiting_handoff.operation_id()
        || pair.engine_id() != awaiting_handoff.engine_id()
        || pair.docker_root_digest() != awaiting_handoff.docker_root_digest()
        || pair.metadata_volume_name() != awaiting_handoff.metadata_volume_name()
        || pair.metadata_volume_id() != awaiting_handoff.metadata_volume_id()
        || pair.layout_digest() != awaiting_handoff.layout_digest()
        || pair.runner_container_id() == pair.dind_container_id()
        || metadata_handoff.connection_id() != awaiting_handoff.connection_id()
        || metadata_handoff.helper_id() != awaiting_handoff.container_id()
        || metadata_handoff.layout_digest() != awaiting_handoff.layout_digest()
    {
        return Err(HostError::Identity);
    }

    journal
        .record_storage_initializer_pair_mounted(metadata_handoff, pair)
        .await?;

    if pending.release_phase() == InitializerReleasePhase::Held {
        // Set this before writing: a failed write can still have delivered the ACK.
        pending.set_release_phase(InitializerReleasePhase::AckAttempted);
        let deadline = Instant::now()
            .checked_add(HANDOFF_LIMIT)
            .ok_or(HostError::DockerTimeout)?;
        process::finish(
            docker,
            journal,
            claim,
            started,
            &mut pending.attached,
            deadline,
        )
        .await?;
        pending.set_release_phase(InitializerReleasePhase::AckCompleted);
    } else if pending.release_phase() != InitializerReleasePhase::AckCompleted {
        return Err(HostError::Identity);
    }

    let removed = remove(docker, journal, claim, started).await?;
    if let Err(error) = journal
        .record_storage_initializer_removed(claim, &removed)
        .await
    {
        journal
            .mark_storage_initializer_removal_uncertain(claim)
            .await?;
        return Err(error);
    }
    let final_readiness = journal
        .metadata_initialization_ready(claim.reservation_id())
        .await?
        .ok_or(HostError::Journal)?;
    let ready =
        VerifiedStorageInitializationReady::after_final_readiness(pending, final_readiness)?;
    pending_slot.take().ok_or(HostError::Identity)?;
    Ok(ready)
}
