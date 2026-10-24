use bollard::container::AttachContainerResults;
use std::cell::Cell;

use crate::journal::{StorageInitializationClaim, VerifiedMetadataInitializationHandoff};

use super::{
    ObservedStorageInitialization,
    proof::{VerifiedStorageInitializerAwaitingHandoff, VerifiedStorageInitializerStarted},
};

/// Non-cloneable initializer operation retained until both job containers mount its volumes.
pub(crate) struct PendingStorageInitialization {
    pub(super) claim: StorageInitializationClaim,
    pub(super) started: VerifiedStorageInitializerStarted,
    pub(super) attached: AttachContainerResults,
    observed: ObservedStorageInitialization,
    awaiting_handoff: VerifiedStorageInitializerAwaitingHandoff,
    metadata_handoff: VerifiedMetadataInitializationHandoff,
    release_phase: Cell<InitializerReleasePhase>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InitializerReleasePhase {
    Held,
    AckAttempted,
    AckCompleted,
}

impl PendingStorageInitialization {
    pub(crate) fn claim(&self) -> &StorageInitializationClaim {
        &self.claim
    }

    pub(super) fn new(
        claim: StorageInitializationClaim,
        started: VerifiedStorageInitializerStarted,
        attached: AttachContainerResults,
        observed: ObservedStorageInitialization,
        awaiting_handoff: VerifiedStorageInitializerAwaitingHandoff,
        metadata_handoff: VerifiedMetadataInitializationHandoff,
    ) -> Self {
        Self {
            claim,
            started,
            attached,
            observed,
            awaiting_handoff,
            metadata_handoff,
            release_phase: Cell::new(InitializerReleasePhase::Held),
        }
    }

    pub(crate) fn observed(&self) -> &ObservedStorageInitialization {
        &self.observed
    }

    pub(crate) fn awaiting_handoff(&self) -> &VerifiedStorageInitializerAwaitingHandoff {
        &self.awaiting_handoff
    }

    pub(crate) fn metadata_handoff(&self) -> &VerifiedMetadataInitializationHandoff {
        &self.metadata_handoff
    }

    pub(super) fn release_phase(&self) -> InitializerReleasePhase {
        self.release_phase.get()
    }

    pub(super) fn set_release_phase(&self, phase: InitializerReleasePhase) {
        self.release_phase.set(phase);
    }
}

impl std::ops::Deref for PendingStorageInitialization {
    type Target = ObservedStorageInitialization;

    fn deref(&self) -> &Self::Target {
        self.observed()
    }
}
