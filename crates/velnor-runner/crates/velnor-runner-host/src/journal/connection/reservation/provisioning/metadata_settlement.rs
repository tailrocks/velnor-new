//! Post-cleanup evidence for the shared engine metadata charge.

use std::time::Instant;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::reservation::provisioning::admission;
use crate::journal::connection::storage::in_transaction;
use crate::journal::resource_probe::ProbeSeed;

use super::super::storage_budget::{StorageAdmissionSample, StorageMedium};

const MAX_SETTLEMENT_GENERATION: i64 = 1024;

/// One journal record that connects a worker capacity reservation to a post-cleanup probe.
pub(crate) struct StorageMetadataSettlementClaim {
    connection_id: String,
    reservation_id: i64,
    generation: i64,
    operation_id: i64,
    scale_set_id: i64,
    instance_id: String,
    probe_operation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    engine_id: String,
    docker_root_digest: String,
    instance_binding_digest: String,
    claimed_at: Instant,
}

impl StorageMetadataSettlementClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn probe_operation_id(&self) -> &str {
        &self.probe_operation_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }
}

/// Result of a fresh sample for the exact cleaned probe operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataSettlementResult {
    /// Other held charges and the configured metadata floor fit.
    Settled,
    /// The journal recorded the sample, but it keeps the full capacity reservation charged.
    HeldAtCapacity,
}

include!("metadata_settlement/claim.rs");
include!("metadata_settlement/record.rs");
include!("metadata_settlement/validation.rs");
