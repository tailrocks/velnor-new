use std::time::Instant;

use bollard::Docker;

use crate::journal::{
    ConnectionJournal, RecordedStorageNetwork, VerifiedMetadataInitializationHandoff,
};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::storage_initialization::PendingStorageInitialization;

use super::super::super::super::adapter::PreparedAcquiredBootstrap;

pub(crate) struct BootstrapPairInput<'a> {
    pub(crate) prepared: &'a PreparedAcquiredBootstrap,
    pub(crate) network: &'a RecordedStorageNetwork,
    pub(crate) metadata: &'a VerifiedMetadataInitializationHandoff,
    pub(crate) pending_initialization: &'a PendingStorageInitialization,
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a ConnectionJournal,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
}

pub(super) struct PairCreateContext<'a> {
    pub(super) prepared: &'a PreparedAcquiredBootstrap,
    pub(super) docker: &'a Docker,
    pub(super) journal: &'a ConnectionJournal,
    pub(super) engine: &'a SelectedEngineContext,
    pub(super) images: &'a VerifiedWorkerImages,
    pub(super) names: &'a crate::worker::WorkerIdentityNames,
    pub(super) deadline: Instant,
}

pub(crate) struct PairStartContext<'a> {
    pub(super) docker: &'a Docker,
    pub(super) journal: &'a ConnectionJournal,
    pub(super) engine: &'a SelectedEngineContext,
    pub(super) images: &'a VerifiedWorkerImages,
    pub(super) metadata: &'a VerifiedMetadataInitializationHandoff,
    pub(super) deadline: Instant,
}

impl<'a> PairStartContext<'a> {
    pub(in crate::launch::bootstrap::containers::pair) fn new(
        docker: &'a Docker,
        journal: &'a ConnectionJournal,
        engine: &'a SelectedEngineContext,
        images: &'a VerifiedWorkerImages,
        metadata: &'a VerifiedMetadataInitializationHandoff,
        deadline: Instant,
    ) -> Self {
        Self {
            docker,
            journal,
            engine,
            images,
            metadata,
            deadline,
        }
    }
}
