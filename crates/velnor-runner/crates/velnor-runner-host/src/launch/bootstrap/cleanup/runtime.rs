// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, EngineContext, Journal, MetadataSettlementResult,
        StorageCleanupIdentity, StorageCleanupObserverWitness,
    },
    launch::{ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages},
};
use bollard::Docker;

use super::{observer, production::ObservedDockerCleanupAbsence};

const CLEANUP_OBSERVER_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct NativeCleanupRuntime {
    docker: Docker,
    journal: Journal,
    engine_context: EngineContext,
    selected: SelectedEngineContext,
    images: VerifiedWorkerImages,
    release_secret: Arc<ReleaseReadSecret>,
}

impl NativeCleanupRuntime {
    pub(crate) fn new(
        docker: Docker,
        journal: Journal,
        engine_context: EngineContext,
        selected: SelectedEngineContext,
        images: VerifiedWorkerImages,
        release_secret: Arc<ReleaseReadSecret>,
    ) -> Result<Self, HostError> {
        if !selected.matches_images(&images) || !selected.matches_selection(&engine_context) {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            docker,
            journal,
            engine_context,
            selected,
            images,
            release_secret,
        })
    }

    pub(crate) async fn prepare_storage_cleanup(
        &self,
        scope: &ConnectionJournal,
        reservation_id: i64,
    ) -> Result<PendingStorageCleanupWitness, HostError> {
        self.images
            .verify_for_engine(&self.docker, &self.selected)
            .await?;
        let identity = scope
            .storage_cleanup_identity(reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        let witness = scope
            .storage_cleanup_witness(reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        validate_scope(
            scope,
            &self.selected,
            &self.engine_context,
            &identity,
            &witness,
        )?;
        Ok(PendingStorageCleanupWitness { identity, witness })
    }

    pub(crate) async fn verify_storage_cleanup(
        &self,
        scope: &ConnectionJournal,
        pending: PendingStorageCleanupWitness,
    ) -> Result<super::VerifiedStorageCleanup, HostError> {
        let deadline = Instant::now() + CLEANUP_OBSERVER_TIMEOUT;
        self.images
            .verify_for_engine(&self.docker, &self.selected)
            .await?;
        let current = scope
            .storage_cleanup_identity(pending.identity.reservation_id())
            .await?
            .ok_or(HostError::Ownership)?;
        let witness = scope
            .storage_cleanup_witness(pending.identity.reservation_id())
            .await?
            .ok_or(HostError::Ownership)?;
        if current != pending.identity || !same_witness(&witness, &pending.witness) {
            return Err(HostError::Ownership);
        }
        validate_scope(
            scope,
            &self.selected,
            &self.engine_context,
            &current,
            &witness,
        )?;

        let before = super::production::observe_docker_cleanup_absence(
            &self.docker,
            &self.engine_context,
            &current,
        )
        .await?;
        let observer_context = crate::launch::resource_probe::CleanupObserverContext {
            docker: &self.docker,
            journal: &self.journal,
            scope,
            selected: &self.selected,
            release_read_secret: self.release_secret.as_ref(),
        };

        let guest = crate::launch::resource_probe::observe_cleanup_binding(
            observer_context,
            pending.identity.reservation_id(),
            witness.binding(),
            deadline,
        )
        .await?;
        let after = super::production::observe_docker_cleanup_absence(
            &self.docker,
            &self.engine_context,
            &current,
        )
        .await?;
        if !same_docker_absence(&before, &after) {
            return Err(HostError::Ownership);
        }
        observer::verified_storage_cleanup(&current, &witness, &after, &guest)
    }

    pub(crate) async fn remove_owned_network(
        &self,
        scope: &ConnectionJournal,
        reservation_id: i64,
    ) -> Result<(), HostError> {
        crate::launch::bootstrap::remove_owned_network(
            &self.docker,
            scope,
            &self.selected,
            reservation_id,
        )
        .await
    }

    pub(crate) async fn settle_storage_metadata_after_cleanup(
        &self,
        scope: &ConnectionJournal,
        reservation_id: i64,
    ) -> Result<MetadataSettlementResult, HostError> {
        crate::launch::resource_probe::settle_storage_metadata_after_cleanup(
            &self.docker,
            &self.journal,
            scope,
            reservation_id,
            &self.selected,
            self.release_secret.as_ref(),
        )
        .await
    }
}

pub(crate) struct PendingStorageCleanupWitness {
    identity: StorageCleanupIdentity,
    witness: StorageCleanupObserverWitness,
}

impl PendingStorageCleanupWitness {
    pub(crate) fn identity(&self) -> &StorageCleanupIdentity {
        &self.identity
    }
}

fn validate_scope(
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    engine_context: &EngineContext,
    identity: &StorageCleanupIdentity,
    witness: &StorageCleanupObserverWitness,
) -> Result<(), HostError> {
    if !selected.matches_selection(engine_context)
        || selected.engine_id() != identity.engine_id()
        || selected.docker_root_digest() != identity.docker_root_digest()
        || witness.connection_id() != scope.connection_id().as_str()
    {
        return Err(HostError::Ownership);
    }
    super::request::validate_witness(identity, witness)
}

fn same_witness(
    left: &StorageCleanupObserverWitness,
    right: &StorageCleanupObserverWitness,
) -> bool {
    left.reservation_id() == right.reservation_id()
        && left.connection_id() == right.connection_id()
        && left.operation_id() == right.operation_id()
        && left.engine_id() == right.engine_id()
        && left.docker_root_digest() == right.docker_root_digest()
        && left.runner_container_id() == right.runner_container_id()
        && left.dind_container_id() == right.dind_container_id()
        && left.instance_binding_digest() == right.instance_binding_digest()
        && left.binding_sha256() == right.binding_sha256()
        && left.binding() == right.binding()
}

fn same_docker_absence(
    left: &ObservedDockerCleanupAbsence,
    right: &ObservedDockerCleanupAbsence,
) -> bool {
    left.reservation_id() == right.reservation_id()
        && left.operation_id() == right.operation_id()
        && left.engine_id() == right.engine_id()
        && left.docker_root_digest() == right.docker_root_digest()
        && left.runner_container_id() == right.runner_container_id()
        && left.dind_container_id() == right.dind_container_id()
        && left.volume_inventory() == right.volume_inventory()
        && left.absent_container_ids() == right.absent_container_ids()
        && left.absent_container_names() == right.absent_container_names()
        && left.absent_volumes() == right.absent_volumes()
}
