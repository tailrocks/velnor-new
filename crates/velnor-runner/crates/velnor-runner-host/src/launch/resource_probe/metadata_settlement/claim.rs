use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, Journal, ProbeSeed, ResourceProbePurpose,
        StorageMetadataSettlementClaim, StorageProfile,
    },
};

use super::super::lifecycle::Deadline;
use super::super::projection::{DockerRoot, ProbeProjection};
use super::setup::SettlementSetup;

pub(super) struct PendingSettlement {
    pub(super) profile: StorageProfile,
    pub(super) evidence: super::super::provider::ProbeImagePreparationEvidence,
    pub(super) engine_id: String,
    pub(super) root: DockerRoot,
    pub(super) architecture: String,
    pub(super) cpu_count: u32,
    pub(super) memory_total: u64,
    pub(super) deadline: Deadline,
    pub(super) projection: ProbeProjection,
    pub(super) claim: StorageMetadataSettlementClaim,
}

pub(super) async fn prepare_settlement_claim(
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    setup: SettlementSetup,
) -> Result<PendingSettlement, HostError> {
    let SettlementSetup {
        profile,
        image,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
    } = setup;
    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let instance_id = deadline.within(journal.instance_id()).await??;
    let projection = ProbeProjection::build(
        operation_id,
        instance_id.clone(),
        engine_id.clone(),
        root.path(),
        image,
    )?;
    let seed = ProbeSeed {
        operation_id: projection.operation_id.clone(),
        instance_id,
        engine_id: projection.engine_id.clone(),
        docker_root_digest: projection.root.digest().to_owned(),
        source_revision: projection.image.source_revision().to_owned(),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: projection.image.binding_fingerprint().to_owned(),
        operation_name: projection.name.clone(),
        projection_digest: projection.projection_digest.clone(),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    };
    let claim = deadline
        .within(scope.prepare_storage_metadata_settlement_probe(reservation_id, seed))
        .await??;
    if claim.probe_operation_id() != projection.operation_id
        || claim.engine_id() != engine_id
        || claim.docker_root_digest() != root.digest()
        || claim.profile_generation_id() != profile.generation_id()
        || claim.profile_digest() != profile.profile_digest()
    {
        let _aborted = deadline
            .within(journal.abort_prepared_probe(&projection.operation_id))
            .await;
        return Err(HostError::Identity);
    }
    Ok(PendingSettlement {
        profile,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
        projection,
        claim,
    })
}
