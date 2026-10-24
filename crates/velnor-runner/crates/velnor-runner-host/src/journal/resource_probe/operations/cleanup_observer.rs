//! Prepare and validate cleanup observer probe operations.

use super::encode_hex;
use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::journal::Journal;
use crate::journal::PreparedCleanupObserverProbe;
use crate::journal::connection::storage::in_transaction;
use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest;

use super::{
    HostHelperRole, ProbeLeaseState, ProbePhase, ProbeRow, ProbeSeed, ProbeWork,
    ResourceProbePurpose, lease,
};
use super::{cleanup_request_valid, ensure_no_active, seed_valid, validate_binding};

impl Journal {
    /// Record one request from a saved worker cleanup binding.
    pub(crate) async fn prepare_cleanup_observer_probe(
        &self,
        scope: &ConnectionJournal,
        reservation_id: i64,
        seed: ProbeSeed,
    ) -> Result<PreparedCleanupObserverProbe, HostError> {
        validate_prepare_input(self, scope, reservation_id, &seed)?;
        let witness = scope
            .storage_cleanup_witness(reservation_id)
            .await?
            .ok_or(HostError::Identity)?;
        validate_witness_seed(&witness, &seed)?;
        let request = CleanupObserverRequest {
            schema_version: 1,
            operation_id: seed.operation_id.clone(),
            binding_sha256: witness.binding_sha256().to_owned(),
            binding: witness.binding().clone(),
        };
        let request_json = canonical_request(&request)?;
        if cleanup_request_sha256(request_json.as_bytes()) != seed.request_sha256
            || !cleanup_request_valid(&seed, &request_json)
        {
            return Err(HostError::Identity);
        }
        persist_probe(self, scope, reservation_id, &seed, &witness, &request_json).await?;
        Ok(prepared_probe(seed, request, request_json))
    }
}

fn validate_prepare_input(
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    seed: &ProbeSeed,
) -> Result<(), HostError> {
    if scope.journal_ref() != journal
        || reservation_id <= 0
        || seed.purpose != ResourceProbePurpose::CleanupObserver
        || !seed_valid(seed)
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

fn validate_witness_seed(
    witness: &crate::journal::StorageCleanupObserverWitness,
    seed: &ProbeSeed,
) -> Result<(), HostError> {
    if witness.engine_id() == seed.engine_id
        && witness.docker_root_digest() == seed.docker_root_digest
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn persist_probe(
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    seed: &ProbeSeed,
    witness: &crate::journal::StorageCleanupObserverWitness,
    request_json: &str,
) -> Result<(), HostError> {
    in_transaction(journal, async |connection| {
        scope.validate_scope(connection).await?;
        validate_binding(connection, seed).await?;
        ensure_no_active(connection).await?;
        validate_witness_link(
            connection,
            reservation_id,
            scope.connection_id().as_str(),
            witness.operation_id(),
            witness.binding_sha256(),
        )
        .await?;
        insert_cleanup_probe(connection, reservation_id, seed, witness, request_json).await?;
        Ok(())
    })
    .await
}

async fn insert_cleanup_probe(
    connection: &turso::Connection,
    reservation_id: i64,
    seed: &ProbeSeed,
    witness: &crate::journal::StorageCleanupObserverWitness,
    request_json: &str,
) -> Result<(), HostError> {
    lease::prepare(connection, seed).await?;
    connection
        .execute(
            "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared', 'resource-probe', 'cleanup-observer', ?10, ?11)",
            (seed.operation_id.as_str(), seed.instance_id.as_str(), seed.engine_id.as_str(), seed.docker_root_digest.as_str(), seed.source_revision.as_str(), seed.runtime_image_id.as_str(), seed.image_binding_digest.as_str(), seed.operation_name.as_str(), seed.projection_digest.as_str(), seed.request_sha256.as_str(), request_json),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_cleanup_observer_probes (probe_operation_id, reservation_id, connection_id, operation_id, binding_sha256) VALUES (?1, ?2, ?3, ?4, ?5)",
            (seed.operation_id.as_str(), reservation_id, witness.connection_id(), witness.operation_id(), witness.binding_sha256()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn prepared_probe(
    seed: ProbeSeed,
    request: CleanupObserverRequest,
    request_json: String,
) -> PreparedCleanupObserverProbe {
    let probe = ProbeRow {
        operation_id: seed.operation_id,
        instance_id: seed.instance_id,
        engine_id: seed.engine_id,
        docker_root_digest: seed.docker_root_digest,
        source_revision: seed.source_revision,
        runtime_image_id: seed.runtime_image_id,
        image_binding_digest: seed.image_binding_digest,
        operation_name: seed.operation_name,
        role: "resource-probe".to_owned(),
        helper_role: HostHelperRole::ResourceProbe,
        work: ProbeWork::ResourceProbe {
            purpose: ResourceProbePurpose::CleanupObserver,
            request_sha256: seed.request_sha256,
            request_context: Some(request_json.clone()),
        },
        projection_digest: seed.projection_digest,
        phase: ProbePhase::Prepared,
        container_id: None,
        lease_state: ProbeLeaseState::Reserved,
    };
    PreparedCleanupObserverProbe {
        request,
        request_json,
        probe,
    }
}

async fn validate_witness_link(
    connection: &turso::Connection,
    reservation_id: i64,
    connection_id: &str,
    operation_id: i64,
    binding_sha256: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses AS witness JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN connection_intents AS intent ON intent.id = provisioning.operation_id AND intent.connection_id = reservation.connection_id WHERE witness.reservation_id = ?1 AND witness.connection_id = ?2 AND witness.operation_id = ?3 AND witness.binding_sha256 = ?4 AND reservation.connection_id = ?2 AND reservation.intent_id = ?3 AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND intent.state IN ('pending', 'uncertain', 'done') AND intent.cleanup_proven = 0 AND state.bootstrap_start_state = 'started' AND state.resources_bound = 1 AND state.storage_cleanup_proven = 0 AND state.instance_binding_digest = witness.instance_binding_digest AND state.guest_boot_id = witness.guest_boot_id AND state.runner_container_id = witness.runner_container_id AND state.dind_container_id = witness.dind_container_id",
            (reservation_id, connection_id, operation_id, binding_sha256),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        && rows.next().await.map_err(|_| HostError::Journal)?.is_none()
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

pub(super) async fn validate_cleanup_probe_link(
    connection: &turso::Connection,
    probe: &ProbeRow,
    request_sha256: &str,
    request_json: &str,
) -> Result<(), HostError> {
    let request: CleanupObserverRequest =
        serde_json::from_str(request_json).map_err(|_| HostError::Journal)?;
    let binding_json = canonical_request_binding(&request)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_cleanup_observer_probes AS cleanup_probe JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = cleanup_probe.reservation_id JOIN resource_probe_operations AS probe ON probe.operation_id = cleanup_probe.probe_operation_id JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN connection_intents AS intent ON intent.id = provisioning.operation_id AND intent.connection_id = reservation.connection_id WHERE cleanup_probe.probe_operation_id = ?1 AND cleanup_probe.binding_sha256 = ?2 AND cleanup_probe.connection_id = witness.connection_id AND cleanup_probe.operation_id = witness.operation_id AND witness.binding_sha256 = ?2 AND witness.binding_json = ?3 AND witness.engine_id = ?4 AND witness.docker_root_digest = ?5 AND probe.probe_purpose = 'cleanup-observer' AND probe.request_sha256 = ?6 AND probe.request_context = ?7 AND reservation.engine_id = ?4 AND reservation.docker_root_digest = ?5 AND reservation.connection_id = witness.connection_id AND reservation.intent_id = witness.operation_id AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND intent.state IN ('pending', 'uncertain', 'done') AND intent.cleanup_proven = 0 AND state.resources_bound = 1 AND state.storage_cleanup_proven = 0 AND state.instance_binding_digest = witness.instance_binding_digest AND state.guest_boot_id = witness.guest_boot_id AND state.runner_container_id = witness.runner_container_id AND state.dind_container_id = witness.dind_container_id",
            (
                probe.operation_id.as_str(),
                request.binding_sha256.as_str(),
                binding_json.as_str(),
                probe.engine_id.as_str(),
                probe.docker_root_digest.as_str(),
                request_sha256,
                request_json,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if request.operation_id != probe.operation_id
        || rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

fn canonical_request_binding(request: &CleanupObserverRequest) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(&request.binding).map_err(|_| HostError::Journal)?;
    String::from_utf8(bytes).map_err(|_| HostError::Journal)
}

pub(super) fn cleanup_request_sha256(request_json: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-cleanup-observer-request-v1\0");
    hasher.update(request_json);
    hasher.update(b"\n");
    encode_hex(&hasher.finalize())
}

fn canonical_request(request: &CleanupObserverRequest) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(request).map_err(|_| HostError::Journal)?;
    if bytes.len() > 32768 {
        return Err(HostError::Journal);
    }
    String::from_utf8(bytes).map_err(|_| HostError::Journal)
}
