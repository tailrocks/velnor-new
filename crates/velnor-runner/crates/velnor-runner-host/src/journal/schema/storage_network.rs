//! Durable identity and state for one private worker network.

use super::sql_normalization::normalize;
use crate::error::HostError;

const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_networks (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL UNIQUE, engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), network_name TEXT NOT NULL UNIQUE CHECK (length(network_name) BETWEEN 1 AND 128), labels_json TEXT NOT NULL CHECK (length(labels_json) BETWEEN 2 AND 2048), create_state TEXT NOT NULL CHECK (create_state IN ('requested', 'created', 'uncertain')), network_id TEXT CHECK (network_id IS NULL OR (length(network_id) = 64 AND network_id NOT GLOB '*[^0-9a-f]*')), remove_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (remove_state IN ('not_requested', 'requested', 'removed', 'uncertain')), CHECK ((create_state = 'created' AND network_id IS NOT NULL) OR (create_state != 'created' AND network_id IS NULL)))";

const NETWORK_TRANSITION_V32: &str = "CREATE TRIGGER IF NOT EXISTS reservation_network_transition BEFORE UPDATE ON capacity_reservation_networks WHEN NOT ((OLD.create_state = NEW.create_state AND OLD.network_id IS NEW.network_id) OR (OLD.create_state = 'requested' AND NEW.create_state IN ('created', 'uncertain')) OR (OLD.create_state = 'uncertain' AND NEW.create_state = 'created')) OR NOT ((OLD.remove_state = NEW.remove_state) OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'requested') OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'removed' AND NEW.create_state IN ('requested', 'uncertain')) OR (OLD.remove_state = 'requested' AND NEW.remove_state IN ('removed', 'uncertain')) OR (OLD.remove_state = 'uncertain' AND NEW.remove_state = 'removed')) BEGIN SELECT RAISE(ABORT, 'invalid private network state change'); END";

const PRE_JIT_RELEASE_NETWORK_GUARD: &str = "OLD.remove_state = 'not_requested' AND NEW.remove_state = 'removed' AND OLD.create_state = 'created' AND NEW.create_state = 'created' AND EXISTS (SELECT 1 FROM physical_attempt_current AS active_attempt JOIN physical_attempts AS attempt ON attempt.lineage_id = active_attempt.lineage_id AND attempt.generation = active_attempt.generation AND attempt.reservation_id = active_attempt.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = active_attempt.reservation_id JOIN connection_intents AS intent ON intent.id = active_attempt.intent_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id JOIN capacity_reservation_pre_jit_cleanup_claims AS claim ON claim.reservation_id = reservation.id JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_settlements AS settlement ON settlement.reservation_id = claim.reservation_id AND settlement.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_probes AS prepared ON prepared.reservation_id = claim.reservation_id AND prepared.claim_generation = claim.claim_generation JOIN resource_probe_operations AS probe ON probe.operation_id = prepared.probe_operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 AND current_profile.generation_id = settlement.profile_generation_id JOIN host_storage_profiles AS profile ON profile.generation_id = current_profile.generation_id WHERE active_attempt.reservation_id = OLD.reservation_id AND active_attempt.intent_id = OLD.operation_id AND attempt.connection_id = OLD.connection_id AND attempt.engine_id = OLD.engine_id AND attempt.docker_root_digest = OLD.docker_root_digest AND reservation.state = 'uncertain' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.state = 'uncertain' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND intent.github_runner_id IS NULL AND auth.acquire_state = 'acquired' AND NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = intent.id) AND NOT EXISTS (SELECT 1 FROM execution_publication_operations AS publication WHERE publication.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM connection_execution_proofs AS proof WHERE proof.reservation_id = reservation.id) AND claim.connection_id = OLD.connection_id AND claim.operation_id = OLD.operation_id AND claim.engine_id = OLD.engine_id AND claim.docker_root_digest = OLD.docker_root_digest AND claim.phase = 'claimed' AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation AND progress.phase = 'network_absent' AND settlement.connection_id = claim.connection_id AND settlement.operation_id = claim.operation_id AND settlement.engine_id = claim.engine_id AND settlement.docker_root_digest = claim.docker_root_digest AND settlement.snapshot_sha256 = claim.snapshot_sha256 AND settlement.claim_generation = claim.claim_generation AND settlement.outcome = 'capacity_available' AND settlement.profile_digest = profile.profile_digest AND prepared.probe_operation_id = settlement.probe_operation_id AND prepared.profile_generation_id = settlement.profile_generation_id AND prepared.profile_digest = settlement.profile_digest AND probe.probe_purpose = 'capacity' AND probe.phase = 'removed' AND probe.engine_id = OLD.engine_id AND probe.docker_root_digest = OLD.docker_root_digest AND lease.state = 'released' AND lease.engine_id = OLD.engine_id AND lease.docker_root_digest = OLD.docker_root_digest AND EXISTS (SELECT 1 FROM capacity_reservation_pre_jit_create_responses AS response WHERE response.reservation_id = reservation.id AND response.connection_id = OLD.connection_id AND response.operation_id = OLD.operation_id AND response.engine_id = OLD.engine_id AND response.docker_root_digest = OLD.docker_root_digest AND response.role = 'runner' AND response.client_api_major = 1 AND response.client_api_minor = 53 AND response.response_status IN (400, 404, 409)) AND (SELECT COUNT(*) FROM capacity_reservation_pre_jit_cleanup_settlement_pools AS sampled WHERE sampled.reservation_id = settlement.reservation_id AND sampled.claim_generation = settlement.claim_generation) = (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS reserved_pool WHERE reserved_pool.reservation_id = reservation.id AND reserved_pool.scope = 'shared-disk' AND reserved_pool.medium = 'disk'))";

const NETWORK_TRANSITION_V33_PREFIX: &str = "CREATE TRIGGER IF NOT EXISTS reservation_network_transition BEFORE UPDATE ON capacity_reservation_networks WHEN NOT ((OLD.create_state = NEW.create_state AND OLD.network_id IS NEW.network_id) OR (OLD.create_state = 'requested' AND NEW.create_state IN ('created', 'uncertain')) OR (OLD.create_state = 'uncertain' AND NEW.create_state = 'created')) OR NOT ((OLD.remove_state = NEW.remove_state) OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'requested') OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'removed' AND NEW.create_state IN ('requested', 'uncertain')) OR (OLD.remove_state = 'requested' AND NEW.remove_state IN ('removed', 'uncertain')) OR (OLD.remove_state = 'uncertain' AND NEW.remove_state = 'removed') OR (";
const NETWORK_TRANSITION_V33_SUFFIX: &str =
    ")) BEGIN SELECT RAISE(ABORT, 'invalid private network state change'); END";

fn network_transition_v33() -> String {
    format!(
        "{NETWORK_TRANSITION_V33_PREFIX}{PRE_JIT_RELEASE_NETWORK_GUARD}{NETWORK_TRANSITION_V33_SUFFIX}"
    )
}

const TRIGGERS: [(&str, &str); 6] = [
    (
        "reservation_network_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_insert_guard BEFORE INSERT ON capacity_reservation_networks WHEN NEW.network_name != (SELECT 'velnor-' || i.runner_name || '-private' FROM connection_intents AS i WHERE i.id = NEW.operation_id AND i.connection_id = NEW.connection_id) OR NEW.labels_json != '{\"io.velnor.connection-id\":\"' || NEW.connection_id || '\",\"io.velnor.engine-id\":\"' || NEW.engine_id || '\",\"io.velnor.managed\":\"true\",\"io.velnor.operation-id\":\"' || NEW.operation_id || '\",\"io.velnor.reservation-id\":\"' || NEW.reservation_id || '\"}' OR NOT EXISTS (SELECT 1 FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id WHERE r.id = NEW.reservation_id AND r.connection_id = NEW.connection_id AND r.intent_id = NEW.operation_id AND p.operation_id = NEW.operation_id AND p.engine_id = NEW.engine_id AND p.docker_root_digest = NEW.docker_root_digest AND r.state = 'reserved' AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0) BEGIN SELECT RAISE(ABORT, 'invalid private network claim'); END",
    ),
    (
        "reservation_network_identity_immutable",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_identity_immutable BEFORE UPDATE OF reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json ON capacity_reservation_networks BEGIN SELECT RAISE(ABORT, 'immutable private network identity'); END",
    ),
    (
        "reservation_network_no_delete",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_no_delete BEFORE DELETE ON capacity_reservation_networks BEGIN SELECT RAISE(ABORT, 'immutable private network record'); END",
    ),
    ("reservation_network_transition", NETWORK_TRANSITION_V32),
    (
        "reservation_network_start_guard",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_start_guard BEFORE UPDATE OF bootstrap_start_state ON capacity_reservation_provisioning_state WHEN OLD.bootstrap_start_state != 'started' AND NEW.bootstrap_start_state = 'started' AND EXISTS (SELECT 1 FROM connection_request_reservations AS request_link WHERE request_link.reservation_id = NEW.reservation_id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_networks AS n WHERE n.reservation_id = NEW.reservation_id AND n.create_state = 'created' AND n.remove_state = 'not_requested' AND n.network_id IS NOT NULL) BEGIN SELECT RAISE(ABORT, 'private network is not recorded'); END",
    ),
    (
        "reservation_network_release_guard",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_release_guard BEFORE UPDATE OF state ON capacity_reservations WHEN NEW.state = 'released' AND EXISTS (SELECT 1 FROM capacity_reservation_networks AS n WHERE n.reservation_id = NEW.id AND n.remove_state != 'removed') BEGIN SELECT RAISE(ABORT, 'private network is not proven absent'); END",
    ),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, statement) in TRIGGERS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn upgrade_for_v33(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'reservation_network_transition'",
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
    if normalize(&actual) != normalize(NETWORK_TRANSITION_V32) {
        return Err(HostError::Journal);
    }
    connection
        .execute("DROP TRIGGER reservation_network_transition", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let transition = network_transition_v33();
    connection
        .execute(&transition, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn restore_v32_transition_for_test(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute("DROP TRIGGER reservation_network_transition", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(NETWORK_TRANSITION_V32, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    validate_with_transition(connection, NETWORK_TRANSITION_V32).await
}

pub(super) async fn validate_v33(connection: &turso::Connection) -> Result<(), HostError> {
    let transition = network_transition_v33();
    validate_with_transition(connection, &transition).await
}

async fn validate_with_transition(
    connection: &turso::Connection,
    expected_transition: &str,
) -> Result<(), HostError> {
    for (name, expected) in
        std::iter::once(("capacity_reservation_networks", TABLE)).chain(TRIGGERS)
    {
        let expected = if name == "reservation_network_transition" {
            expected_transition
        } else {
            expected
        };
        let kind = if name == "capacity_reservation_networks" {
            "table"
        } else {
            "trigger"
        };
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, name),
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
        let actual = normalize(&actual);
        let expected = normalize(expected);
        if actual != expected {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
