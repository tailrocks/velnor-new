//! This module adds the cleanup observer probe schema.

use super::sql_normalization::normalize;
use crate::error::HostError;
use sha2::{Digest, Sha256};
use turso::Connection;
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupObserverBinding, CleanupObserverRequest,
};

#[path = "cleanup_observer_witness/recovering.rs"]
mod recovering;
pub(super) use recovering::{upgrade_for_v32, validate_current};

const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_cleanup_observer_witnesses (reservation_id INTEGER PRIMARY KEY REFERENCES capacity_reservations(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), operation_id INTEGER NOT NULL REFERENCES connection_intents(id), engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), runner_container_id TEXT NOT NULL CHECK (length(runner_container_id) BETWEEN 12 AND 128), dind_container_id TEXT NOT NULL CHECK (length(dind_container_id) BETWEEN 12 AND 128 AND dind_container_id != runner_container_id), instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'), guest_boot_id TEXT NOT NULL CHECK (length(guest_boot_id) = 36), binding_json TEXT NOT NULL CHECK (length(CAST(binding_json AS BLOB)) BETWEEN 1 AND 32768), binding_sha256 TEXT NOT NULL CHECK (length(binding_sha256) = 64 AND binding_sha256 NOT GLOB '*[^0-9a-f]*'))";
const PROBE_TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_cleanup_observer_probes (probe_operation_id TEXT PRIMARY KEY REFERENCES resource_probe_operations(operation_id), reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_cleanup_observer_witnesses(reservation_id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), operation_id INTEGER NOT NULL REFERENCES connection_intents(id), binding_sha256 TEXT NOT NULL CHECK (length(binding_sha256) = 64 AND binding_sha256 NOT GLOB '*[^0-9a-f]*'))";

const INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_witness_insert_guard BEFORE INSERT ON capacity_reservation_cleanup_observer_witnesses WHEN NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN connection_intents AS intent ON intent.id = provisioning.operation_id AND intent.connection_id = reservation.connection_id WHERE reservation.id = NEW.reservation_id AND reservation.connection_id = NEW.connection_id AND reservation.intent_id = NEW.operation_id AND reservation.engine_id = NEW.engine_id AND reservation.docker_root_digest = NEW.docker_root_digest AND reservation.state = 'reserved' AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.launch_phase = 'bootstrap_started' AND provisioning.operation_id = NEW.operation_id AND state.bootstrap_start_state = 'started' AND state.runner_container_id = NEW.runner_container_id AND state.dind_container_id = NEW.dind_container_id AND state.resources_bound = 0 AND state.instance_binding_digest IS NULL AND state.jit_state = 'not_requested' AND state.guest_boot_id IS NULL) BEGIN SELECT RAISE(ABORT, 'invalid cleanup observer witness binding'); END";

const IMMUTABLE_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_witness_no_update BEFORE UPDATE ON capacity_reservation_cleanup_observer_witnesses BEGIN SELECT RAISE(ABORT, 'cleanup observer witness is immutable'); END";
const IMMUTABLE_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_witness_no_delete BEFORE DELETE ON capacity_reservation_cleanup_observer_witnesses BEGIN SELECT RAISE(ABORT, 'cleanup observer witness is retained'); END";
const BOUND_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_resources_bound_guard BEFORE UPDATE OF resources_bound ON capacity_reservation_provisioning_state WHEN OLD.resources_bound = 0 AND NEW.resources_bound = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses AS witness WHERE witness.reservation_id = NEW.reservation_id AND witness.instance_binding_digest = NEW.instance_binding_digest AND witness.guest_boot_id = NEW.guest_boot_id AND witness.runner_container_id = NEW.runner_container_id AND witness.dind_container_id = NEW.dind_container_id) BEGIN SELECT RAISE(ABORT, 'cleanup observer witness is required'); END";
const JIT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_jit_guard BEFORE UPDATE OF jit_state ON capacity_reservation_provisioning_state WHEN OLD.jit_state = 'not_requested' AND NEW.jit_state = 'requested' AND OLD.resources_bound = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses AS witness WHERE witness.reservation_id = NEW.reservation_id AND witness.instance_binding_digest = NEW.instance_binding_digest AND witness.guest_boot_id = NEW.guest_boot_id AND witness.runner_container_id = NEW.runner_container_id AND witness.dind_container_id = NEW.dind_container_id) BEGIN SELECT RAISE(ABORT, 'cleanup observer witness is required'); END";

pub(super) const PROBE_INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_probe_insert_guard BEFORE INSERT ON capacity_reservation_cleanup_observer_probes WHEN NOT EXISTS (SELECT 1 FROM resource_probe_operations AS probe JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = NEW.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE probe.operation_id = NEW.probe_operation_id AND probe.helper_role = 'resource-probe' AND probe.role = 'resource-probe' AND probe.probe_purpose = 'cleanup-observer' AND probe.request_sha256 IS NOT NULL AND probe.request_context IS NOT NULL AND probe.phase = 'prepared' AND probe.container_id IS NULL AND probe.engine_id = witness.engine_id AND probe.docker_root_digest = witness.docker_root_digest AND lease.state = 'reserved' AND lease.engine_id = probe.engine_id AND lease.docker_root_digest = probe.docker_root_digest AND reservation.state IN ('reserved', 'uncertain') AND reservation.connection_id = NEW.connection_id AND reservation.intent_id = NEW.operation_id AND witness.connection_id = NEW.connection_id AND witness.operation_id = NEW.operation_id AND witness.binding_sha256 = NEW.binding_sha256 AND provisioning.operation_id = NEW.operation_id AND state.resources_bound = 1 AND state.storage_cleanup_proven = 0 AND state.instance_binding_digest = witness.instance_binding_digest AND state.guest_boot_id = witness.guest_boot_id AND state.runner_container_id = witness.runner_container_id AND state.dind_container_id = witness.dind_container_id) BEGIN SELECT RAISE(ABORT, 'invalid cleanup observer probe binding'); END";

const PROBE_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_probe_no_update BEFORE UPDATE ON capacity_reservation_cleanup_observer_probes BEGIN SELECT RAISE(ABORT, 'cleanup observer probe binding is immutable'); END";
const PROBE_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_probe_no_delete BEFORE DELETE ON capacity_reservation_cleanup_observer_probes BEGIN SELECT RAISE(ABORT, 'cleanup observer probe binding is retained'); END";
const REQUEST_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_request_no_update BEFORE UPDATE OF probe_purpose, request_sha256, request_context ON resource_probe_operations WHEN (OLD.probe_purpose = 'cleanup-observer' OR NEW.probe_purpose = 'cleanup-observer') AND (NEW.probe_purpose IS NOT OLD.probe_purpose OR NEW.request_sha256 IS NOT OLD.request_sha256 OR NEW.request_context IS NOT OLD.request_context) BEGIN SELECT RAISE(ABORT, 'cleanup observer request is immutable'); END";
const REQUEST_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_request_no_delete BEFORE DELETE ON resource_probe_operations WHEN OLD.probe_purpose = 'cleanup-observer' BEGIN SELECT RAISE(ABORT, 'cleanup observer request is retained'); END";

const CLEANUP_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS cleanup_observer_cleanup_proof_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses AS witness JOIN capacity_reservation_cleanup_observer_probes AS cleanup_probe ON cleanup_probe.reservation_id = witness.reservation_id JOIN resource_probe_operations AS probe ON probe.operation_id = cleanup_probe.probe_operation_id WHERE witness.reservation_id = NEW.reservation_id AND probe.probe_purpose = 'cleanup-observer' AND probe.phase = 'removed') BEGIN SELECT RAISE(ABORT, 'cleanup observer proof is required'); END";

const TRIGGERS: [(&str, &str); 11] = [
    ("cleanup_observer_witness_insert_guard", INSERT_GUARD),
    ("cleanup_observer_witness_no_update", IMMUTABLE_UPDATE),
    ("cleanup_observer_witness_no_delete", IMMUTABLE_DELETE),
    ("cleanup_observer_resources_bound_guard", BOUND_GUARD),
    ("cleanup_observer_jit_guard", JIT_GUARD),
    ("cleanup_observer_probe_insert_guard", PROBE_INSERT_GUARD),
    ("cleanup_observer_probe_no_update", PROBE_NO_UPDATE),
    ("cleanup_observer_probe_no_delete", PROBE_NO_DELETE),
    ("cleanup_observer_request_no_update", REQUEST_NO_UPDATE),
    ("cleanup_observer_request_no_delete", REQUEST_NO_DELETE),
    ("cleanup_observer_cleanup_proof_guard", CLEANUP_GUARD),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(PROBE_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, trigger) in TRIGGERS {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate(connection).await
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_cleanup_observer_witnesses'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize(&actual) != normalize(TABLE) {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_cleanup_observer_probes'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize(&actual) != normalize(PROBE_TABLE) {
        return Err(HostError::Journal);
    }
    for (name, expected) in TRIGGERS.into_iter().filter(|(name, _)| !name.is_empty()) {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [name],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?;
        let current_probe_guard = name == "cleanup_observer_probe_insert_guard"
            && recovering::is_v32_probe_insert_guard(&actual)?;
        if normalize(&actual) != normalize(expected) && !current_probe_guard {
            return Err(HostError::Journal);
        }
    }
    validate_rows(connection).await
}

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_cleanup_observer_witnesses AS witness JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = witness.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = witness.reservation_id WHERE witness.connection_id != reservation.connection_id OR witness.operation_id != provisioning.operation_id OR witness.operation_id != reservation.intent_id OR witness.engine_id != reservation.engine_id OR witness.docker_root_digest != reservation.docker_root_digest OR witness.runner_container_id != state.runner_container_id OR witness.dind_container_id != state.dind_container_id OR witness.instance_binding_digest IS NOT state.instance_binding_digest OR witness.guest_boot_id IS NOT state.guest_boot_id OR length(CAST(witness.binding_json AS BLOB)) > 32768 OR length(witness.binding_sha256) != 64 OR witness.binding_sha256 GLOB '*[^0-9a-f]*'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count == 0 {
        validate_witness_payloads(connection).await?;
        validate_probe_rows(connection).await
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_witness_payloads(connection: &Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT binding_json, binding_sha256, guest_boot_id FROM capacity_reservation_cleanup_observer_witnesses",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let json: String = row.get(0).map_err(|_| HostError::Journal)?;
        let digest: String = row.get(1).map_err(|_| HostError::Journal)?;
        let boot_id: String = row.get(2).map_err(|_| HostError::Journal)?;
        if json.len() > 32768 || binding_digest(json.as_bytes()) != digest {
            return Err(HostError::Journal);
        }
        let binding: CleanupObserverBinding =
            serde_json::from_str(&json).map_err(|_| HostError::Journal)?;
        velnor_resource_probe::cleanup_observer_wire::validate_binding(&binding)
            .map_err(|_| HostError::Journal)?;
        if serde_json::to_vec(&binding).map_err(|_| HostError::Journal)? != json.as_bytes()
            || binding.expected_guest_boot_id != boot_id
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_probe_rows(connection: &Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM resource_probe_operations AS probe LEFT JOIN capacity_reservation_cleanup_observer_probes AS cleanup_probe ON cleanup_probe.probe_operation_id = probe.operation_id LEFT JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = cleanup_probe.reservation_id LEFT JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id WHERE probe.probe_purpose = 'cleanup-observer' AND (cleanup_probe.probe_operation_id IS NULL OR witness.reservation_id IS NULL OR probe.role != 'resource-probe' OR probe.helper_role != 'resource-probe' OR probe.request_context IS NULL OR probe.request_sha256 IS NULL OR probe.engine_id != witness.engine_id OR probe.docker_root_digest != witness.docker_root_digest OR cleanup_probe.connection_id != witness.connection_id OR cleanup_probe.operation_id != witness.operation_id OR cleanup_probe.binding_sha256 != witness.binding_sha256 OR reservation.connection_id != witness.connection_id OR reservation.intent_id != witness.operation_id OR reservation.engine_id != witness.engine_id OR reservation.docker_root_digest != witness.docker_root_digest)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count != 0 {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_cleanup_observer_probes AS cleanup_probe JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = cleanup_probe.reservation_id JOIN resource_probe_operations AS probe ON probe.operation_id = cleanup_probe.probe_operation_id WHERE cleanup_probe.connection_id != witness.connection_id OR cleanup_probe.operation_id != witness.operation_id OR cleanup_probe.binding_sha256 != witness.binding_sha256 OR probe.probe_purpose != 'cleanup-observer' OR probe.request_context IS NULL OR probe.request_sha256 IS NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count != 0 {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT probe.operation_id, probe.request_sha256, probe.request_context, witness.binding_sha256, witness.binding_json FROM capacity_reservation_cleanup_observer_probes AS cleanup_probe JOIN resource_probe_operations AS probe ON probe.operation_id = cleanup_probe.probe_operation_id JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = cleanup_probe.reservation_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let operation_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let digest: String = row.get(1).map_err(|_| HostError::Journal)?;
        let request: String = row.get(2).map_err(|_| HostError::Journal)?;
        let binding_sha256: String = row.get(3).map_err(|_| HostError::Journal)?;
        let binding_json: String = row.get(4).map_err(|_| HostError::Journal)?;
        if !crate::journal::resource_probe::operations::cleanup_request_matches(
            &operation_id,
            &request,
            &digest,
        ) {
            return Err(HostError::Journal);
        }
        let parsed: CleanupObserverRequest =
            serde_json::from_str(&request).map_err(|_| HostError::Journal)?;
        if parsed.binding_sha256 != binding_sha256
            || serde_json::to_vec(&parsed.binding).map_err(|_| HostError::Journal)?
                != binding_json.as_bytes()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn binding_digest(json: &[u8]) -> String {
    crate::digest_hex::lowercase_hex(&Sha256::digest(json))
}
