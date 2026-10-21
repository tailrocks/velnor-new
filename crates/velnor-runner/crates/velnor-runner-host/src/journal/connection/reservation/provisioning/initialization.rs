//! Durable storage initializer effects.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::worker::storage_initialization::{
    ObservedStorageInitialization, StorageInitializationImage, StorageInitializationLimits,
    StorageInitializerCreateResponse, VerifiedStorageInitializerAbsent,
    VerifiedStorageInitializerCreated, VerifiedStorageInitializerRemoved,
    VerifiedStorageInitializerStarted,
};

use super::{hold_context, hold_is_claimable, mark_hold_uncertain};

/// Evidence about a Docker create request when the initializer stops.
pub(crate) enum CreateInterruptionOutcome {
    NoRequestEntered,
    RequestOutcomeUnknown,
    TerminalResponseReceived,
}

/// Durable permission for one storage initializer generation.
#[derive(Clone)]
pub(crate) struct StorageInitializationClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    helper_name: String,
    engine_id: String,
    docker_root_digest: String,
    image_runtime_id: String,
    image_source_sha: String,
    home_volume: String,
    work_volume: String,
    layout_version: i64,
    metadata_volume: Option<String>,
    metadata_volume_id: Option<String>,
    nano_cpus: i64,
    memory_bytes: i64,
    pids_limit: i64,
    helper_id: Option<String>,
    start_state: String,
    result_state: String,
    remove_state: String,
    abort_helper_id: Option<String>,
    abort_claim_generation: i64,
}

impl StorageInitializationClaim {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    pub(crate) fn helper_name(&self) -> &str {
        &self.helper_name
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn image_runtime_id(&self) -> &str {
        &self.image_runtime_id
    }
    pub(crate) fn image_source_sha(&self) -> &str {
        &self.image_source_sha
    }
    pub(crate) fn home_volume(&self) -> &str {
        &self.home_volume
    }
    pub(crate) fn work_volume(&self) -> &str {
        &self.work_volume
    }
    pub(crate) const fn layout_version(&self) -> i64 {
        self.layout_version
    }
    pub(crate) fn metadata_volume_name(&self) -> Option<&str> {
        self.metadata_volume.as_deref()
    }
    pub(crate) fn metadata_volume_id(&self) -> Option<&str> {
        self.metadata_volume_id.as_deref()
    }
    pub(crate) const fn nano_cpus(&self) -> i64 {
        self.nano_cpus
    }
    pub(crate) const fn memory_bytes(&self) -> i64 {
        self.memory_bytes
    }
    pub(crate) const fn pids_limit(&self) -> i64 {
        self.pids_limit
    }
    pub(crate) fn helper_id(&self) -> Option<&str> {
        self.helper_id.as_deref()
    }
    pub(crate) fn abort_helper_id(&self) -> Option<&str> {
        self.abort_helper_id.as_deref()
    }
    pub(crate) const fn abort_claim_generation(&self) -> i64 {
        self.abort_claim_generation
    }
}

include!("initialization/claims.rs");
include!("initialization/claim_interruption.rs");
include!("initialization/lifecycle.rs");
include!("initialization/recovery.rs");
include!("initialization/helpers.rs");
include!("initialization/metadata.rs");

#[cfg(test)]
mod commit_gate;
#[cfg(test)]
pub(crate) use commit_gate::{CreateCommitStage, arm_create_commit_gate};
