//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

#[cfg(test)]
pub(crate) use connection::ReservationSnapshot;

pub(crate) mod capacity_header;
#[path = "journal_completion.rs"]
mod completion;
mod connection;
mod doctor_probe;
pub(crate) mod execution_authorization;
mod file_identity;
pub(crate) mod header_authority;
mod image_import;
mod launch;
mod read;
#[cfg(not(test))]
pub(in crate::journal) mod request_lifecycle;
#[cfg(test)]
pub(crate) mod request_lifecycle;
mod resource_probe;
mod scale_set_create;
mod schema;
mod session_refresh;
mod source_authorization;
mod sql;
mod transaction;
mod worker_volume;

use sql::{intent_row, one_row, same_ids, token_rejected};

#[path = "journal/file_backed.rs"]
mod file_backed;
use file_backed::live_id;
pub use file_backed::{IntentState, Journal, Outcome};

#[cfg(test)]
#[path = "journal/connection_tests.rs"]
mod connection_tests;
#[cfg(test)]
#[path = "journal/migration_tests.rs"]
mod migration_tests;
#[cfg(test)]
mod schema_metadata_tests;
pub(crate) use capacity_header::{HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt};
pub(crate) use completion::{
    CleanupClaim, CompletedLaunch, CompletionIdentity, CompletionInboxEntry,
    MAX_COMPLETION_BODY_BYTES, MAX_COMPLETION_INBOX_SCAN,
};
pub(crate) use connection::JobFactsPublicationClaim;
pub(crate) use connection::ListenerDeliveryClaim;
pub(crate) use connection::NativeCompletionReservation;
pub(crate) use connection::TerminalReplayBinding;
pub(crate) use connection::{
    PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation, ReservationState,
};

pub(crate) use connection::{
    AssignmentIdentity, AssignmentKey, BootstrapContainerCreateClaim, BootstrapStartClaim,
    BoundJobFactsSnapshot, CapacityLimits, ConnectionCompletionInboxEntry, ConnectionJournal,
    CreateInterruptionOutcome, EngineContext, MetadataSettlementResult, OperationalCapacityLimits,
    RecordedStorageNetwork, RequestPhysicalReservationResult, ReservationRequest,
    SessionCreateState, SessionRefreshClaim, SourceAuthorizationBinding, StorageAdmissionSample,
    StorageCleanupIdentity, StorageCleanupObserverWitness, StorageFilesystemKind,
    StorageInitializationClaim, StorageInodePolicy, StorageMedium, StorageMetadataSettlementClaim,
    StorageNetworkCreateClaim, StorageNetworkRemovalClaim, StorageObjectCreateClaim,
    StorageObserver, StoragePoolLimit, StoragePoolObservation, StorageProfile,
    StorageProvisioningAdmission, StorageProvisioningMemoryObservation, StorageProvisioningPlan,
    StorageProvisioningRole, StorageRole, StorageRoleLimit, StorageSampleIdentity,
    VerifiedMetadataInitialization, VerifiedMetadataInitializationHandoff,
};
#[cfg(test)]
pub(crate) use connection::{CreateCommitStage, arm_create_commit_gate};
#[cfg(test)]
pub(crate) use connection::{
    ReservationResult, StorageAdmission, StorageCleanupCgroup, StorageCleanupMount,
};
#[cfg(test)]
pub(crate) use connection::{
    SessionRefreshState, fixture_admission, fixture_profile, fixture_provisioning_profile,
    fixture_storage_provisioning_admission,
    fixture_storage_provisioning_admission_with_filesystem_digest,
};
#[cfg(test)]
pub(crate) use connection::{StorageCacheProjection, StorageReservationPlan, StorageRoleCharge};
#[cfg(test)]
pub(crate) use doctor_probe::DoctorProbeRequestRecord;
pub(crate) use doctor_probe::{
    DoctorProbeOutcome, DoctorProbePrepareResult, DoctorProbeStartResult, DoctorProbeState,
    DoctorProbeTicket,
};
pub(crate) use image_import::{
    ImageImportDescriptor, ImageImportIdentity, ImageImportInspection, ImageImportObservation,
    ImageImportRequest, ImageImportState,
};
pub(crate) use scale_set_create::{
    CreatedScaleSetResponse, ExistingScaleSetObservation, ScaleSetCreateNoEffect,
    ScaleSetCreateRecord, ScaleSetCreateRequest, ScaleSetCreateState,
};
pub(crate) use source_authorization::AssignedAcquireResult;
pub(crate) use source_authorization::SourceAuthorizationResult;

pub(crate) use request_lifecycle::{
    AcquiredRecoveryCandidate, AuthorizedOffer, RequestAcquireState,
};
#[cfg(test)]
pub(crate) use resource_probe::ProbeLeaseState;
pub(crate) use resource_probe::capacity_request_sha256;
pub(crate) use resource_probe::{
    ExecutionPublicationContext, ExecutionPublicationParent, ExecutionPublicationRecoveryRecord,
    ExecutionPublicationSeed, HostHelperRole, PreparedCleanupObserverProbe, ProbePhase, ProbeRow,
    ProbeSeed, ProbeWork, PublicationKind, ResourceProbePurpose,
};
