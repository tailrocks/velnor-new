//! Shared capacity accounting and one-use admission.

mod admission;
mod enumerate;
pub(in crate::journal) mod jit_conflict;
mod lifecycle;
mod limits;
mod physical_attempt;
pub(in crate::journal) mod post_jit_retirement;
mod provisioning;
mod worker_health;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalWorkerHealthCandidate {
    connection_id: String,
    scale_set_id: i64,
    assignment: crate::journal::connection::AssignmentIdentity,
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    physical_nonce: String,
    binding_digest: String,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalWorkerExitDisposition {
    Recorded,
    AlreadyRecorded,
    IgnoredStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalWorkerExitKind {
    Exited { exit_code: i32 },
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalWorkerExitObservation {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    scale_set_id: i64,
    runner_container_id: String,
    kind: LocalWorkerExitKind,
    observed_at_unix_ms: u64,
}

pub(crate) use jit_conflict::JitConflictRecoveryClaim;
pub(in crate::journal::connection::reservation) use jit_conflict::{
    conflict_recovery_metadata_claim_ready, has_conflict_recovery_claim,
    record_conflict_metadata_settlement,
};
pub(crate) use post_jit_retirement::{
    PostJitRetirementCandidate, PostJitRetirementClaim, PreparedRetirementRemoteAbsence,
};

pub(in crate::journal) use provisioning::validate_retirement_digest;

pub(in crate::journal) async fn release_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, crate::error::HostError> {
    provisioning::release_ready(connection, reservation_id).await
}

pub(in crate::journal) async fn record_no_acquire_outcome(
    connection: &turso::Connection,
    reservation_id: i64,
    response_digest: &str,
) -> Result<(), crate::error::HostError> {
    physical_attempt::record_no_acquire_outcome(connection, reservation_id, response_digest).await
}

pub(in crate::journal) async fn record_terminal_cleanup_outcome(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    intent_id: i64,
) -> Result<bool, crate::error::HostError> {
    physical_attempt::record_terminal_cleanup_outcome(
        connection,
        connection_id,
        reservation_id,
        intent_id,
    )
    .await
}
mod read;
mod storage;
mod storage_budget;

/// These host capacity totals use the shared journal validation path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadinessCapacityUsage {
    pub(crate) jobs: u32,
    pub(crate) cpu_millicores: u64,
    pub(crate) memory_bytes: u64,
    pub(crate) max_jobs: u32,
    pub(crate) max_cpu_millicores: u64,
    pub(crate) max_memory_bytes: u64,
    pub(crate) operational_cpu_millicores: u64,
    pub(crate) operational_memory_bytes: u64,
    pub(crate) operational_lease_quiescent: bool,
}

impl ReadinessCapacityUsage {
    /// Check charged work against worker limits.
    /// Check that one measured guest capacity sample covers those limits plus the operational reserve.
    pub(crate) fn valid_for_guest(
        &self,
        effective_cpu_millicores: u64,
        memory_total_bytes: u64,
        memory_available_bytes: u64,
    ) -> bool {
        self.max_jobs > 0
            && self.max_cpu_millicores > 0
            && self.max_memory_bytes > 0
            && self.operational_cpu_millicores > 0
            && self.operational_memory_bytes > 0
            && self.operational_lease_quiescent
            && self.jobs <= self.max_jobs
            && self.cpu_millicores <= self.max_cpu_millicores
            && self.memory_bytes <= self.max_memory_bytes
            && self
                .max_cpu_millicores
                .checked_add(self.operational_cpu_millicores)
                .is_some_and(|required| required <= effective_cpu_millicores)
            && self
                .max_memory_bytes
                .checked_add(self.operational_memory_bytes)
                .is_some_and(|required| required <= memory_total_bytes)
            && memory_available_bytes >= self.operational_memory_bytes
    }
}

impl crate::journal::Journal {
    pub(crate) async fn readiness_capacity_usage(
        &self,
    ) -> Result<ReadinessCapacityUsage, crate::error::HostError> {
        super::storage::read_transaction(self, async |connection| {
            let usage = storage::capacity_usage(connection).await?;
            let limits = storage::capacity_limits(connection).await?;
            let operational_lease_quiescent = operational_lease_quiescent(connection).await?;
            Ok(ReadinessCapacityUsage {
                jobs: usage.jobs,
                cpu_millicores: usage.cpu_millicores,
                memory_bytes: usage.memory_bytes,
                max_jobs: limits.jobs,
                max_cpu_millicores: limits.cpu_millicores,
                max_memory_bytes: limits.memory_bytes,
                operational_cpu_millicores: limits.operational_cpu_millicores,
                operational_memory_bytes: limits.operational_memory_bytes,
                operational_lease_quiescent,
            })
        })
        .await
    }
}

async fn operational_lease_quiescent(
    connection: &turso::Connection,
) -> Result<bool, crate::error::HostError> {
    let mut rows = connection
        .query(
            "SELECT state FROM host_operation_leases WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| crate::error::HostError::Journal)?;
    let Some(row) = rows
        .next()
        .await
        .map_err(|_| crate::error::HostError::Journal)?
    else {
        return Ok(true);
    };
    match row
        .get::<String>(0)
        .map_err(|_| crate::error::HostError::Journal)?
        .as_str()
    {
        "released" => Ok(true),
        "reserved" | "active" | "uncertain" | "recovering" => Ok(false),
        _ => Err(crate::error::HostError::Journal),
    }
}

pub(crate) use enumerate::{
    PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation,
};
#[cfg(test)]
pub(crate) use read::ReservationSnapshot;
pub(crate) use read::ReservationState;

#[cfg(test)]
#[path = "reservation/storage_budget_tests.rs"]
mod storage_budget_tests;

#[cfg(test)]
#[path = "reservation/storage_budget_samples_tests.rs"]
mod storage_budget_samples_tests;

#[cfg(test)]
#[path = "reservation/network_tests.rs"]
mod network_tests;

pub(crate) use admission::ReservationRequest;
#[cfg(test)]
pub(crate) use admission::ReservationResult;
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use provisioning::JobFactsPublicationClaim;
pub(crate) use provisioning::ListenerDeliveryClaim;
pub(crate) use provisioning::MetadataSettlementResult;
pub(crate) use provisioning::PreJitCapacityProbeClaim;
pub(crate) use provisioning::PreJitCleanupClaim;
pub(crate) use provisioning::PreJitCleanupSettlementResult;
pub(crate) use provisioning::StorageCleanupObserverWitness;
pub(crate) use provisioning::{
    BootstrapContainerCreateClaim, BootstrapStartClaim, RecordedStorageNetwork,
};
#[cfg(test)]
pub(crate) use provisioning::{CreateCommitStage, arm_create_commit_gate};
pub(crate) use provisioning::{
    CreateInterruptionOutcome, PhysicalSuccessorReservationResult,
    RequestPhysicalReservationResult, StorageCleanupIdentity, StorageInitializationClaim,
    StorageMetadataSettlementClaim, StorageNetworkCreateClaim, StorageNetworkRemovalClaim,
    StorageObjectCreateClaim, StorageProvisioningPlan, StorageProvisioningRole,
    VerifiedMetadataInitialization, VerifiedMetadataInitializationHandoff,
};
pub(crate) use provisioning::{
    PreJitBootstrapContainer, PreJitCleanupInventory, PreJitCreateSpec, PreJitInitializerIdentity,
    PreJitNetworkIdentity, PreJitStorageObject,
};
#[cfg(test)]
pub(crate) use storage_budget::StorageAdmission;
pub(crate) use storage_budget::{
    BoundJobFactsSnapshot, StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy,
    StorageMedium, StorageObserver, StoragePoolLimit, StoragePoolObservation,
    StoragePoolObservationInput, StorageProfile, StorageProvisioningAdmission,
    StorageProvisioningMemoryObservation, StorageRole, StorageRoleLimit, StorageSampleIdentity,
};
#[cfg(test)]
pub(crate) use storage_budget_tests::{
    fixture_admission, fixture_profile, fixture_provisioning_profile,
    fixture_storage_provisioning_admission, fixture_storage_provisioning_admission_for_profile,
};

pub(super) async fn validate_storage_budget_schema(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    storage_budget::validate_schema(connection).await
}

pub(in crate::journal) async fn insert_metadata_settlement_claim(
    connection: &turso::Connection,
    scope: &super::ConnectionJournal,
    reservation_id: i64,
    seed: &crate::journal::resource_probe::ProbeSeed,
) -> Result<StorageMetadataSettlementClaim, crate::error::HostError> {
    provisioning::insert_metadata_settlement_claim(connection, scope, reservation_id, seed).await
}

pub(in crate::journal) async fn record_metadata_probe_cleanup(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), crate::error::HostError> {
    provisioning::record_metadata_probe_cleanup(
        connection,
        operation_id,
        engine_id,
        docker_root_digest,
        operation_name,
        container_id,
    )
    .await
}

pub(in crate::journal) async fn resources_bound(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, crate::error::HostError> {
    provisioning::resources_bound(connection, connection_id, intent_id).await
}

pub(in crate::journal) async fn validate_jit_conflict_settlement_digests(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    jit_conflict::validate_conflict_settlement_digests(connection).await
}
