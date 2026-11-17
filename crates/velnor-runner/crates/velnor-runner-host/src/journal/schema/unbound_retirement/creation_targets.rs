// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

mod history;
mod observations;
mod predicates;
use predicates::{CONTAINERS, NETWORKS, RESOURCE_MATCH, VOLUMES};

const TABLE: &str = "CREATE TABLE capacity_reservation_unbound_creation_targets (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), resource_kind TEXT NOT NULL CHECK (resource_kind IN ('container', 'volume', 'network')), role TEXT NOT NULL, docker_name TEXT NOT NULL CHECK (length(docker_name) BETWEEN 1 AND 128), docker_id TEXT NOT NULL CHECK (length(docker_id) BETWEEN 1 AND 256), observation_sha256 TEXT NOT NULL CHECK (length(observation_sha256) = 64 AND observation_sha256 NOT GLOB '*[^0-9a-f]*'), observation_json TEXT NOT NULL CHECK (length(CAST(observation_json AS BLOB)) BETWEEN 1 AND 16384 AND json_valid(observation_json)), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), operation_id INTEGER NOT NULL REFERENCES connection_intents(id), engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL, observation_claim_generation INTEGER NOT NULL CHECK (observation_claim_generation > 0), observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0), PRIMARY KEY (reservation_id, resource_kind, role))";

fn objects() -> Vec<(&'static str, String, String)> {
    let mut objects = super::shape::objects();
    objects.push((
        "table",
        "capacity_reservation_unbound_creation_targets".to_owned(),
        TABLE.to_owned(),
    ));
    let identity = "SELECT 1 FROM capacity_reservation_unbound_retirement_claims claim JOIN capacity_reservations r ON r.id=claim.reservation_id AND r.connection_id=claim.connection_id AND r.intent_id=claim.operation_id JOIN physical_attempt_current cur ON cur.reservation_id=r.id AND cur.intent_id=r.intent_id WHERE claim.reservation_id=NEW.reservation_id AND claim.connection_id=NEW.connection_id AND claim.operation_id=NEW.operation_id AND claim.engine_id=NEW.engine_id AND claim.docker_root_digest=NEW.docker_root_digest AND claim.claim_generation=NEW.observation_claim_generation AND claim.lease_until>unixepoch() AND r.state='uncertain'";
    let insert = format!(
        "CREATE TRIGGER unbound_creation_target_insert_guard BEFORE INSERT ON capacity_reservation_unbound_creation_targets WHEN NOT EXISTS ({identity}) OR NOT ({}) BEGIN SELECT RAISE(ABORT, 'Invalid unbound creation observation.'); END",
        RESOURCE_MATCH.replace("{t}", "NEW")
    );
    objects.push((
        "trigger",
        "unbound_creation_target_insert_guard".to_owned(),
        insert,
    ));
    add_owner_guards(&mut objects);
    for action in ["UPDATE", "DELETE"] {
        let name = format!("unbound_creation_target_no_{}", action.to_ascii_lowercase());
        objects.push(("trigger", name.clone(), format!("CREATE TRIGGER {name} BEFORE {action} ON capacity_reservation_unbound_creation_targets BEGIN SELECT RAISE(ABORT, 'Unbound creation observations are immutable.'); END")));
    }
    for (family, predicate) in [
        ("container", CONTAINERS),
        ("volume", VOLUMES),
        ("network", NETWORKS),
    ] {
        for (suffix, table) in [
            (
                "progress",
                "capacity_reservation_unbound_retirement_progress",
            ),
            ("retirement", "physical_attempt_unbound_retirements"),
        ] {
            let name = format!("unbound_{family}_publication_{suffix}_guard");
            objects.push(("trigger", name.clone(), format!("CREATE TRIGGER {name} BEFORE INSERT ON {table} WHEN {} BEGIN SELECT RAISE(ABORT, 'An attempted creation has no publication proof.'); END", predicate.replace("{r}", "NEW.reservation_id"))));
        }
    }
    let name = "unbound_network_publication_removed_guard";
    objects.push(("trigger", name.to_owned(), format!("CREATE TRIGGER {name} BEFORE UPDATE OF remove_state ON capacity_reservation_networks WHEN NEW.remove_state='removed' AND EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims claim WHERE claim.reservation_id=NEW.reservation_id) AND {} BEGIN SELECT RAISE(ABORT, 'The network creation has no publication proof.'); END", NETWORKS.replace("{r}", "NEW.reservation_id"))));
    let name = "unbound_initializer_abort_owner_guard";
    objects.push(("trigger", name.to_owned(), format!("CREATE TRIGGER {name} BEFORE UPDATE ON capacity_reservation_storage_initialization WHEN (NEW.abort_state IS NOT OLD.abort_state OR NEW.abort_claim_generation IS NOT OLD.abort_claim_generation OR NEW.abort_lease_until IS NOT OLD.abort_lease_until) AND (EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims claim WHERE claim.reservation_id=NEW.reservation_id) OR EXISTS (SELECT 1 FROM capacity_reservation_unbound_classifications owner JOIN capacity_reservation_unbound_candidates candidate ON candidate.reservation_id=owner.reservation_id JOIN capacity_reservations reservation ON reservation.id=owner.reservation_id JOIN connection_intents intent ON intent.id=reservation.intent_id WHERE owner.reservation_id=NEW.reservation_id AND reservation.state='uncertain' AND intent.state='uncertain')) BEGIN SELECT RAISE(ABORT, 'Unbound retirement owns this initializer.'); END")));
    let name = "unbound_initializer_claim_owner_guard";
    objects.push(("trigger", name.to_owned(), "CREATE TRIGGER unbound_initializer_claim_owner_guard BEFORE INSERT ON capacity_reservation_unbound_retirement_claims WHEN EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization init WHERE init.reservation_id=NEW.reservation_id AND init.abort_state!='absent' AND (init.abort_state!='not_requested' OR init.abort_lease_until>unixepoch())) BEGIN SELECT RAISE(ABORT, 'Initializer abort owns this reservation.'); END".to_owned()));
    for (suffix, action) in [
        ("insert", "AFTER INSERT"),
        ("update", "AFTER UPDATE"),
        ("delete", "AFTER DELETE"),
    ] {
        let name = format!("capacity_reservation_unbound_creation_targets_revision_{suffix}");
        objects.push((
            "trigger",
            name,
            super::super::jit_conflict_settlement::revision_trigger_sql(
                "capacity_reservation_unbound_creation_targets",
                suffix,
                action,
            ),
        ));
    }
    objects
}

fn add_owner_guards(objects: &mut Vec<(&'static str, String, String)>) {
    let owner = "SELECT 1 FROM capacity_reservation_unbound_owners owner JOIN capacity_reservation_unbound_candidates candidate ON candidate.reservation_id=owner.reservation_id JOIN capacity_reservations reservation ON reservation.id=owner.reservation_id JOIN connection_intents intent ON intent.id=reservation.intent_id WHERE owner.reservation_id=NEW.reservation_id AND reservation.state='uncertain' AND intent.state='uncertain'";
    for (name, table) in [
        (
            "unbound_current_identity_claim_guard",
            "capacity_reservation_unbound_retirement_claims",
        ),
        (
            "unbound_current_identity_target_guard",
            "capacity_reservation_unbound_creation_targets",
        ),
        (
            "unbound_current_identity_progress_guard",
            "capacity_reservation_unbound_retirement_progress",
        ),
        (
            "unbound_current_identity_retirement_guard",
            "physical_attempt_unbound_retirements",
        ),
    ] {
        objects.push(("trigger", name.to_owned(), format!("CREATE TRIGGER {name} BEFORE INSERT ON {table} WHEN NOT EXISTS ({owner}) BEGIN SELECT RAISE(ABORT, 'The current attempt does not own unbound cleanup.'); END")));
    }
}

pub(super) async fn install(conn: &turso::Connection) -> Result<(), HostError> {
    for (_, _, sql) in objects() {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_objects(conn, objects()).await?;
    let invalid = format!(
        "SELECT EXISTS (SELECT 1 FROM capacity_reservation_unbound_creation_targets target JOIN capacity_reservation_unbound_retirement_claims claim ON claim.reservation_id=target.reservation_id WHERE target.connection_id!=claim.connection_id OR target.operation_id!=claim.operation_id OR target.engine_id!=claim.engine_id OR target.docker_root_digest!=claim.docker_root_digest OR target.observation_claim_generation>claim.claim_generation OR NOT ({})) OR EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_progress phase WHERE (phase.phase='containers_absent' AND {}) OR (phase.phase='volumes_absent' AND {})) OR EXISTS (SELECT 1 FROM capacity_reservation_networks network WHERE network.remove_state='removed' AND EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims claim WHERE claim.reservation_id=network.reservation_id) AND {}) OR EXISTS (SELECT 1 FROM physical_attempt_unbound_retirements retired WHERE {} OR {} OR {})",
        RESOURCE_MATCH.replace("{t}", "target"),
        CONTAINERS.replace("{r}", "phase.reservation_id"),
        VOLUMES.replace("{r}", "phase.reservation_id"),
        history::unpublished_networks("network.reservation_id"),
        CONTAINERS.replace("{r}", "retired.reservation_id"),
        VOLUMES.replace("{r}", "retired.reservation_id"),
        history::unpublished_networks("retired.reservation_id")
    );
    require_false(conn, &invalid).await?;
    history::validate(conn).await?;
    observations::validate(conn).await
}

pub(super) async fn migrate_history(conn: &turso::Connection) -> Result<(), HostError> {
    history::migrate(conn).await
}

async fn require_false(conn: &turso::Connection, sql: &str) -> Result<(), HostError> {
    let mut rows = conn.query(sql, ()).await.map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}
