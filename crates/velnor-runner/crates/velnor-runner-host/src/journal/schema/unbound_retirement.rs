// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

mod creation_targets;
mod dispatch_deadlines;
mod shape;
pub(in crate::journal) use shape::UNBOUND_SHAPE;

pub(super) async fn migrate_v48_to_v49(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v48(conn).await?;
    creation_targets::migrate_history(conn).await?;
    creation_targets::install(conn).await?;
    replace_shape_guards(conn, UNBOUND_SHAPE).await?;
    conn.execute("PRAGMA user_version = 49", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_objects(conn, objects(UNBOUND_SHAPE)).await?;
    creation_targets::validate(conn).await?;
    if super::journal_version(conn).await? >= 51 {
        dispatch_deadlines::validate(conn).await?;
    }
    Ok(())
}

pub(super) async fn migrate_v49_to_v51(conn: &turso::Connection) -> Result<(), HostError> {
    validate(conn).await?;
    dispatch_deadlines::install(conn).await?;
    conn.execute("PRAGMA user_version = 51", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

const VERSION: i64 = 43;

const CLAIMS: &str = "CREATE TABLE capacity_reservation_unbound_retirement_claims (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservations(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), operation_id INTEGER NOT NULL UNIQUE REFERENCES connection_intents(id), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), claim_generation INTEGER NOT NULL CHECK (claim_generation > 0), lease_until INTEGER NOT NULL CHECK (lease_until > 0))";

const PROGRESS: &str = "CREATE TABLE capacity_reservation_unbound_retirement_progress (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), phase TEXT NOT NULL CHECK (phase IN ('containers_absent', 'volumes_absent')), claim_generation INTEGER NOT NULL CHECK (claim_generation > 0), evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64 AND evidence_sha256 NOT GLOB '*[^0-9a-f]*'), PRIMARY KEY (reservation_id, phase))";

const RETIREMENTS: &str = "CREATE TABLE physical_attempt_unbound_retirements (lineage_id INTEGER NOT NULL, generation INTEGER NOT NULL CHECK (generation > 0), reservation_id INTEGER NOT NULL UNIQUE REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), proof_digest TEXT NOT NULL CHECK (length(proof_digest) = 64 AND proof_digest NOT GLOB '*[^0-9a-f]*'), settlement_digest TEXT NOT NULL CHECK (length(settlement_digest) = 64 AND settlement_digest NOT GLOB '*[^0-9a-f]*'), created_at INTEGER NOT NULL CHECK (created_at >= 0), PRIMARY KEY (lineage_id, generation), CHECK (proof_digest = settlement_digest), FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id))";

const SHAPE_V43: &str = "state.bootstrap_start_state IN ('started', 'uncertain') AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND state.storage_cleanup_proven = 0";

const SHAPE_V44: &str = "{s}.resources_bound = 0 AND {s}.jit_state = 'not_requested' AND {s}.publication_state = 'not_requested' AND {s}.delivery_state = 'not_requested' AND {s}.storage_cleanup_proven = 0 AND (({s}.bootstrap_start_state IN ('started', 'uncertain') AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 2) OR ({s}.bootstrap_start_state = 'not_requested' AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 0 AND EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS init WHERE init.reservation_id = {s}.reservation_id AND init.abort_state = 'absent')))";

const SHAPE_V45: &str = "{s}.resources_bound = 0 AND {s}.jit_state = 'not_requested' AND {s}.publication_state = 'not_requested' AND {s}.delivery_state = 'not_requested' AND {s}.storage_cleanup_proven = 0 AND (({s}.bootstrap_start_state IN ('started', 'uncertain') AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 2) OR ({s}.bootstrap_start_state = 'not_requested' AND ((SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 0 OR ((SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id AND pair_container.create_state = 'unclaimed' AND pair_container.start_state = 'not_requested' AND pair_container.container_id IS NULL) = 2)) AND EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS init WHERE init.reservation_id = {s}.reservation_id AND init.abort_state = 'absent')))";

const CLAIM_INSERT_GUARD: &str = "CREATE TRIGGER unbound_retirement_claim_insert_guard BEFORE INSERT ON capacity_reservation_unbound_retirement_claims WHEN NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id AND provisioning.operation_id = intent.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE reservation.id = NEW.reservation_id AND reservation.connection_id = NEW.connection_id AND intent.id = NEW.operation_id AND provisioning.engine_id = NEW.engine_id AND provisioning.docker_root_digest = NEW.docker_root_digest AND NEW.claim_generation = 1 AND reservation.state = 'uncertain' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.state = 'uncertain' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND {shape}) BEGIN SELECT RAISE(ABORT, 'invalid unbound retirement claim'); END";

const CLAIM_UPDATE_GUARD: &str = "CREATE TRIGGER unbound_retirement_claim_update_guard BEFORE UPDATE ON capacity_reservation_unbound_retirement_claims WHEN NEW.reservation_id != OLD.reservation_id OR NEW.connection_id != OLD.connection_id OR NEW.operation_id != OLD.operation_id OR NEW.engine_id != OLD.engine_id OR NEW.docker_root_digest != OLD.docker_root_digest OR NEW.claim_generation != OLD.claim_generation + 1 BEGIN SELECT RAISE(ABORT, 'invalid unbound retirement claim'); END";

const PROGRESS_INSERT_GUARD: &str = "CREATE TRIGGER unbound_retirement_progress_insert_guard BEFORE INSERT ON capacity_reservation_unbound_retirement_progress WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims AS claim JOIN capacity_reservations AS reservation ON reservation.id = claim.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = claim.reservation_id WHERE claim.reservation_id = NEW.reservation_id AND claim.claim_generation = NEW.claim_generation AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND reservation.state = 'uncertain' AND {shape} AND (NEW.phase = 'containers_absent' OR EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_progress AS prior WHERE prior.reservation_id = NEW.reservation_id AND prior.phase = 'containers_absent'))) BEGIN SELECT RAISE(ABORT, 'unbound retirement phase is not proven'); END";

const RETIREMENT_INSERT_GUARD: &str = "CREATE TRIGGER physical_attempt_unbound_retirement_insert_guard BEFORE INSERT ON physical_attempt_unbound_retirements WHEN NOT EXISTS (SELECT 1 FROM physical_attempt_current AS cur JOIN capacity_reservation_unbound_retirement_claims AS claim ON claim.reservation_id = cur.reservation_id AND claim.operation_id = cur.intent_id JOIN capacity_reservations AS reservation ON reservation.id = cur.reservation_id AND reservation.intent_id = cur.intent_id JOIN connection_intents AS intent ON intent.id = cur.intent_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cur.reservation_id WHERE cur.lineage_id = NEW.lineage_id AND cur.generation = NEW.generation AND cur.reservation_id = NEW.reservation_id AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND reservation.state = 'uncertain' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.state = 'uncertain' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND {shape} AND (SELECT COUNT(*) FROM capacity_reservation_unbound_retirement_progress AS progress WHERE progress.reservation_id = NEW.reservation_id) = 2 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_networks AS network WHERE network.reservation_id = NEW.reservation_id AND network.remove_state != 'removed')) BEGIN SELECT RAISE(ABORT, 'unbound retirement is not proven'); END";

const BIND_FENCE: &str = "CREATE TRIGGER unbound_retirement_bind_fence BEFORE UPDATE OF resources_bound ON capacity_reservation_provisioning_state WHEN NEW.resources_bound = 1 AND EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims AS claim WHERE claim.reservation_id = NEW.reservation_id) BEGIN SELECT RAISE(ABORT, 'A retirement claim prevents this operation.'); END";

const GUARDS_WITH_SHAPE: [&str; 3] = [
    "unbound_retirement_claim_insert_guard",
    "unbound_retirement_progress_insert_guard",
    "physical_attempt_unbound_retirement_insert_guard",
];

const IMMUTABLE: [&str; 5] = [
    "CREATE TRIGGER unbound_retirement_claim_no_delete BEFORE DELETE ON capacity_reservation_unbound_retirement_claims BEGIN SELECT RAISE(ABORT, 'unbound retirement history is immutable'); END",
    "CREATE TRIGGER unbound_retirement_progress_no_update BEFORE UPDATE ON capacity_reservation_unbound_retirement_progress BEGIN SELECT RAISE(ABORT, 'unbound retirement history is immutable'); END",
    "CREATE TRIGGER unbound_retirement_progress_no_delete BEFORE DELETE ON capacity_reservation_unbound_retirement_progress BEGIN SELECT RAISE(ABORT, 'unbound retirement history is immutable'); END",
    "CREATE TRIGGER physical_attempt_unbound_retirements_no_update BEFORE UPDATE ON physical_attempt_unbound_retirements BEGIN SELECT RAISE(ABORT, 'unbound retirement history is immutable'); END",
    "CREATE TRIGGER physical_attempt_unbound_retirements_no_delete BEFORE DELETE ON physical_attempt_unbound_retirements BEGIN SELECT RAISE(ABORT, 'unbound retirement history is immutable'); END",
];

const TABLES: [(&str, &str); 3] = [
    ("capacity_reservation_unbound_retirement_claims", CLAIMS),
    ("capacity_reservation_unbound_retirement_progress", PROGRESS),
    ("physical_attempt_unbound_retirements", RETIREMENTS),
];

const TERMINAL_PROOFS_VIEW: &str = r"CREATE VIEW physical_attempt_terminal_proofs AS
SELECT lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_outcomes
UNION ALL
SELECT lineage_id, generation, reservation_id, 'post_jit_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_post_jit_retirements
UNION ALL
SELECT lineage_id, generation, reservation_id, 'jit_conflict_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_jit_conflict_settlements
UNION ALL
SELECT lineage_id, generation, reservation_id, 'unbound_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_unbound_retirements";

fn objects(shape: &str) -> Vec<(&'static str, String, String)> {
    let shape = shape.replace("{s}", "state");
    let shaped = |sql: &str| sql.replace("{shape}", &shape);
    let mut objects = TABLES
        .into_iter()
        .map(|(name, sql)| ("table", name.to_owned(), sql.to_owned()))
        .collect::<Vec<_>>();
    for sql in [
        shaped(CLAIM_INSERT_GUARD),
        CLAIM_UPDATE_GUARD.to_owned(),
        shaped(PROGRESS_INSERT_GUARD),
        shaped(RETIREMENT_INSERT_GUARD),
        BIND_FENCE.to_owned(),
    ]
    .into_iter()
    .chain(IMMUTABLE.into_iter().map(str::to_owned))
    {
        objects.push(("trigger", trigger_name(&sql).to_owned(), sql));
    }
    for (table, _) in TABLES {
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            objects.push((
                "trigger",
                format!("{table}_revision_{suffix}"),
                super::jit_conflict_settlement::revision_trigger_sql(table, suffix, action),
            ));
        }
    }
    objects.push((
        "view",
        "physical_attempt_terminal_proofs".to_owned(),
        TERMINAL_PROOFS_VIEW.to_owned(),
    ));
    objects
}

pub(super) async fn migrate_v42_to_v43(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v42(conn).await?;
    conn.execute("DROP VIEW physical_attempt_terminal_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, _, sql) in objects(SHAPE_V43) {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("PRAGMA user_version = 43", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v43(conn).await
}

pub(super) async fn migrate_v43_to_v44(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v43(conn).await?;
    replace_shape_guards(conn, SHAPE_V44).await?;
    conn.execute("PRAGMA user_version = 44", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v44(conn).await
}

pub(super) async fn migrate_v44_to_v45(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v44(conn).await?;
    replace_shape_guards(conn, SHAPE_V45).await?;
    conn.execute("PRAGMA user_version = 45", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v45(conn).await
}

async fn replace_shape_guards(conn: &turso::Connection, shape: &str) -> Result<(), HostError> {
    let current = objects(shape);
    for name in GUARDS_WITH_SHAPE {
        conn.execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
        let (_, _, sql) = current
            .iter()
            .find(|(_, candidate, _)| candidate == name)
            .ok_or(HostError::Journal)?;
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn terminal_view_sql(conn: &turso::Connection) -> Result<&'static str, HostError> {
    if super::journal_version(conn).await? >= VERSION {
        Ok(TERMINAL_PROOFS_VIEW)
    } else {
        Ok(super::jit_conflict_settlement::terminal_view_sql())
    }
}

pub(super) async fn validate_v43(conn: &turso::Connection) -> Result<(), HostError> {
    validate_objects(conn, objects(SHAPE_V43)).await
}

pub(super) async fn validate_v44(conn: &turso::Connection) -> Result<(), HostError> {
    validate_objects(conn, objects(SHAPE_V44)).await
}

pub(super) async fn validate_v45(conn: &turso::Connection) -> Result<(), HostError> {
    validate_objects(conn, objects(SHAPE_V45)).await
}

async fn validate_objects(
    conn: &turso::Connection,
    objects: Vec<(&'static str, String, String)>,
) -> Result<(), HostError> {
    for (kind, name, expected) in objects {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, name.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual: String = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::sql_normalization::normalize(&actual)
                != super::sql_normalization::normalize(&expected)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn trigger_name(sql: &str) -> &str {
    sql.strip_prefix("CREATE TRIGGER ")
        .and_then(|body| body.split_ascii_whitespace().next())
        .unwrap_or("")
}
