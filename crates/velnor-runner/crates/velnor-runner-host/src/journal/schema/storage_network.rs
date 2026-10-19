//! Durable identity and state for one private worker network.

use super::sql_normalization::normalize;
use crate::error::HostError;

const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_networks (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL UNIQUE, engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), network_name TEXT NOT NULL UNIQUE CHECK (length(network_name) BETWEEN 1 AND 128), labels_json TEXT NOT NULL CHECK (length(labels_json) BETWEEN 2 AND 2048), create_state TEXT NOT NULL CHECK (create_state IN ('requested', 'created', 'uncertain')), network_id TEXT CHECK (network_id IS NULL OR (length(network_id) = 64 AND network_id NOT GLOB '*[^0-9a-f]*')), remove_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (remove_state IN ('not_requested', 'requested', 'removed', 'uncertain')), CHECK ((create_state = 'created' AND network_id IS NOT NULL) OR (create_state != 'created' AND network_id IS NULL)))";

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
    (
        "reservation_network_transition",
        "CREATE TRIGGER IF NOT EXISTS reservation_network_transition BEFORE UPDATE ON capacity_reservation_networks WHEN NOT ((OLD.create_state = NEW.create_state AND OLD.network_id IS NEW.network_id) OR (OLD.create_state = 'requested' AND NEW.create_state IN ('created', 'uncertain')) OR (OLD.create_state = 'uncertain' AND NEW.create_state = 'created')) OR NOT ((OLD.remove_state = NEW.remove_state) OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'requested') OR (OLD.remove_state = 'not_requested' AND NEW.remove_state = 'removed' AND NEW.create_state IN ('requested', 'uncertain')) OR (OLD.remove_state = 'requested' AND NEW.remove_state IN ('removed', 'uncertain')) OR (OLD.remove_state = 'uncertain' AND NEW.remove_state = 'removed')) BEGIN SELECT RAISE(ABORT, 'invalid private network state change'); END",
    ),
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

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in
        std::iter::once(("capacity_reservation_networks", TABLE)).chain(TRIGGERS)
    {
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
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
