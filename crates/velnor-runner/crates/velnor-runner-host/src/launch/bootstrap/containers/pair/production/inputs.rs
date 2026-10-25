use crate::error::HostError;
use crate::journal::{RecordedStorageNetwork, VerifiedMetadataInitializationHandoff};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::storage_initialization::PendingStorageInitialization;

use super::super::super::super::adapter::PreparedAcquiredBootstrap;

pub(super) fn validate_pair_inputs(
    prepared: &PreparedAcquiredBootstrap,
    network: &RecordedStorageNetwork,
    metadata: &VerifiedMetadataInitializationHandoff,
    pending_initialization: &PendingStorageInitialization,
    journal: &crate::journal::ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(i64, crate::worker::WorkerIdentityNames), HostError> {
    let acquired = prepared.acquired();
    let reservation_id = acquired.reservation_id();
    if reservation_id != network.reservation_id()
        || acquired.intent_id() != network.operation_id()
        || network.engine_id() != engine.engine_id()
        || network.docker_root_digest() != engine.docker_root_digest()
        || metadata.reservation_id() != reservation_id
        || metadata.operation_id() != acquired.intent_id()
        || metadata.connection_id() != journal.connection_id().as_str()
        || metadata.engine_id() != engine.engine_id()
        || metadata.docker_root_digest() != engine.docker_root_digest()
        || metadata.metadata_volume_name() != prepared.metadata().docker_name()
        || metadata.metadata_volume_id() != prepared.metadata().docker_id()
        || pending_initialization.awaiting_handoff().connection_id() != metadata.connection_id()
        || pending_initialization.awaiting_handoff().reservation_id() != reservation_id
        || pending_initialization.awaiting_handoff().operation_id() != acquired.intent_id()
        || pending_initialization.awaiting_handoff().engine_id() != engine.engine_id()
        || pending_initialization
            .awaiting_handoff()
            .docker_root_digest()
            != engine.docker_root_digest()
        || pending_initialization.awaiting_handoff().container_id() != metadata.helper_id()
        || pending_initialization
            .awaiting_handoff()
            .metadata_volume_name()
            != metadata.metadata_volume_name()
        || pending_initialization
            .awaiting_handoff()
            .metadata_volume_id()
            != metadata.metadata_volume_id()
        || pending_initialization.awaiting_handoff().layout_version() != 2
        || pending_initialization.awaiting_handoff().layout_digest() != metadata.layout_digest()
        || metadata.layout_digest().len() != 64
        || !metadata
            .layout_digest()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || metadata.operation_key().is_empty()
        || !engine.matches_images(images)
    {
        return Err(HostError::Identity);
    }
    let names = crate::worker::WorkerIdentityNames::from_nonce(
        acquired
            .runner_name()
            .strip_prefix('v')
            .ok_or(HostError::Identity)?,
    )?;
    Ok((reservation_id, names))
}
