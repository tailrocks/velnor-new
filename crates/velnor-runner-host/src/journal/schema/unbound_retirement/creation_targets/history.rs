// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const TABLE: &str = "CREATE TABLE capacity_reservation_unbound_historical_network_removals (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL, network_name TEXT NOT NULL, labels_json TEXT NOT NULL, create_state TEXT NOT NULL, lineage_id INTEGER NOT NULL, generation INTEGER NOT NULL, proof_digest TEXT NOT NULL, settlement_digest TEXT NOT NULL, created_at INTEGER NOT NULL)";

const IDENTITY: &str = "historical.reservation_id=resource.reservation_id AND historical.connection_id=resource.connection_id AND historical.operation_id=resource.operation_id AND historical.engine_id=resource.engine_id AND historical.docker_root_digest=resource.docker_root_digest AND historical.network_name=resource.network_name AND historical.labels_json=resource.labels_json AND historical.create_state=resource.create_state AND resource.network_id IS NULL AND resource.remove_state='removed'";
const TERMINAL: &str = "retired.reservation_id=historical.reservation_id AND retired.lineage_id=historical.lineage_id AND retired.generation=historical.generation AND retired.proof_digest=historical.proof_digest AND retired.settlement_digest=historical.settlement_digest AND retired.created_at=historical.created_at";
const CLAIM: &str = "claim.reservation_id=resource.reservation_id AND claim.connection_id=resource.connection_id AND claim.operation_id=resource.operation_id AND claim.engine_id=resource.engine_id AND claim.docker_root_digest=resource.docker_root_digest";

pub(super) fn objects() -> Vec<(&'static str, String, String)> {
    let mut objects = vec![(
        "table",
        "capacity_reservation_unbound_historical_network_removals".to_owned(),
        TABLE.to_owned(),
    )];
    for action in ["INSERT", "UPDATE", "DELETE"] {
        let name = format!(
            "unbound_historical_network_no_{}",
            action.to_ascii_lowercase()
        );
        objects.push(("trigger", name.clone(), format!("CREATE TRIGGER {name} BEFORE {action} ON capacity_reservation_unbound_historical_network_removals BEGIN SELECT RAISE(ABORT, 'Historical network removals are immutable.'); END")));
    }
    objects
}

pub(super) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let capture = format!(
        "INSERT INTO capacity_reservation_unbound_historical_network_removals SELECT resource.reservation_id,resource.connection_id,resource.operation_id,resource.engine_id,resource.docker_root_digest,resource.network_name,resource.labels_json,resource.create_state,retired.lineage_id,retired.generation,retired.proof_digest,retired.settlement_digest,retired.created_at FROM capacity_reservation_networks resource JOIN capacity_reservation_unbound_retirement_claims claim ON {CLAIM} JOIN physical_attempt_unbound_retirements retired ON retired.reservation_id=resource.reservation_id WHERE resource.network_id IS NULL AND resource.remove_state='removed'"
    );
    conn.execute(&capture, ())
        .await
        .map_err(|_| HostError::Journal)?;
    normalize_active(conn).await?;
    for (_, _, sql) in objects().into_iter().skip(1) {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn normalize_active(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn.query("SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='reservation_network_transition'", ()).await.map_err(|_| HostError::Journal)?;
    let transition: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    conn.execute("DROP TRIGGER reservation_network_transition", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let reset = format!(
        "UPDATE capacity_reservation_networks AS resource SET remove_state='uncertain' WHERE resource.network_id IS NULL AND resource.remove_state='removed' AND EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims claim JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id AND reservation.connection_id=claim.connection_id AND reservation.intent_id=claim.operation_id JOIN connection_intents intent ON intent.id=claim.operation_id AND intent.connection_id=claim.connection_id JOIN physical_attempt_current current ON current.reservation_id=claim.reservation_id AND current.intent_id=claim.operation_id WHERE {CLAIM} AND reservation.state='uncertain' AND reservation.remote_cleanup_proven=0 AND reservation.local_cleanup_proven=0 AND intent.state='uncertain' AND intent.cleanup_proven=0 AND NOT EXISTS (SELECT 1 FROM physical_attempt_terminal_proofs terminal WHERE terminal.reservation_id=resource.reservation_id))"
    );
    conn.execute(&reset, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(&transition, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) fn unpublished_networks(reservation: &str) -> String {
    let excluded = format!(
        "NOT EXISTS (SELECT 1 FROM capacity_reservation_unbound_historical_network_removals historical JOIN physical_attempt_unbound_retirements retired ON {TERMINAL} WHERE {IDENTITY})"
    );
    super::predicates::NETWORKS
        .replace("{r}", reservation)
        .replacen(
            "resource.network_id IS NULL",
            &format!("resource.network_id IS NULL AND {excluded}"),
            1,
        )
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::super::validate_objects(conn, objects()).await?;
    let invalid = format!(
        "SELECT EXISTS (SELECT 1 FROM capacity_reservation_unbound_historical_network_removals historical WHERE NOT EXISTS (SELECT 1 FROM capacity_reservation_networks resource JOIN capacity_reservation_unbound_retirement_claims claim ON {CLAIM} JOIN physical_attempt_unbound_retirements retired ON {TERMINAL} WHERE {IDENTITY}))"
    );
    super::require_false(conn, &invalid).await
}
