//! This module defines the schema for immutable physical attempt history and the current attempt.

use crate::error::HostError;

const LINEAGE: &str = "physical_attempt_lineages";
const ATTEMPT: &str = "physical_attempts";
const CURRENT: &str = "physical_attempt_current";
const OUTCOME: &str = "physical_attempt_outcomes";

const LINEAGE_DDL: &str = "CREATE TABLE IF NOT EXISTS physical_attempt_lineages (lineage_id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), UNIQUE (lineage_id, connection_id, scale_set_id, binding_digest))";
const ATTEMPT_DDL: &str = "CREATE TABLE IF NOT EXISTS physical_attempts (lineage_id INTEGER NOT NULL REFERENCES physical_attempt_lineages(lineage_id), generation INTEGER NOT NULL CHECK (generation > 0), reservation_id INTEGER NOT NULL UNIQUE REFERENCES capacity_reservations(id), intent_id INTEGER NOT NULL UNIQUE REFERENCES connection_intents(id), physical_nonce TEXT NOT NULL CHECK (length(physical_nonce) = 32 AND physical_nonce NOT GLOB '*[^0-9a-f]*'), connection_id TEXT NOT NULL, scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'), acquire_response_digest TEXT CHECK (acquire_response_digest IS NULL OR (length(acquire_response_digest) = 64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*')), engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), resource_spec_digest TEXT CHECK (resource_spec_digest IS NULL OR (length(resource_spec_digest) = 64 AND resource_spec_digest NOT GLOB '*[^0-9a-f]*')), created_at INTEGER NOT NULL CHECK (created_at >= 0), PRIMARY KEY (lineage_id, generation), UNIQUE (lineage_id, generation, reservation_id), UNIQUE (lineage_id, generation, intent_id), UNIQUE (connection_id, scale_set_id, physical_nonce), FOREIGN KEY (connection_id, scale_set_id, witness_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id), FOREIGN KEY (lineage_id, connection_id, scale_set_id, binding_digest) REFERENCES physical_attempt_lineages(lineage_id, connection_id, scale_set_id, binding_digest))";
const CURRENT_DDL: &str = "CREATE TABLE IF NOT EXISTS physical_attempt_current (lineage_id INTEGER PRIMARY KEY, generation INTEGER NOT NULL CHECK (generation > 0), reservation_id INTEGER NOT NULL UNIQUE, intent_id INTEGER NOT NULL UNIQUE, FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id), FOREIGN KEY (lineage_id, generation, intent_id) REFERENCES physical_attempts(lineage_id, generation, intent_id))";
const OUTCOME_DDL: &str = "CREATE TABLE IF NOT EXISTS physical_attempt_outcomes (lineage_id INTEGER NOT NULL, generation INTEGER NOT NULL CHECK (generation > 0), reservation_id INTEGER NOT NULL, outcome TEXT NOT NULL CHECK (outcome IN ('pre_jit_capacity_available', 'normal_completion', 'acquire_no_effect')), proof_digest TEXT NOT NULL CHECK (length(proof_digest) = 64 AND proof_digest NOT GLOB '*[^0-9a-f]*'), settlement_digest TEXT NOT NULL CHECK (length(settlement_digest) = 64 AND settlement_digest NOT GLOB '*[^0-9a-f]*'), created_at INTEGER NOT NULL CHECK (created_at >= 0), PRIMARY KEY (lineage_id, generation), FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id))";
const ATTEMPT_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempts_no_update BEFORE UPDATE ON physical_attempts BEGIN SELECT RAISE(ABORT, 'physical attempt history is immutable'); END";
const ATTEMPT_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempts_no_delete BEFORE DELETE ON physical_attempts BEGIN SELECT RAISE(ABORT, 'physical attempt history is immutable'); END";
const LINEAGE_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_lineages_no_update BEFORE UPDATE ON physical_attempt_lineages BEGIN SELECT RAISE(ABORT, 'physical attempt lineage is immutable'); END";
const LINEAGE_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_lineages_no_delete BEFORE DELETE ON physical_attempt_lineages BEGIN SELECT RAISE(ABORT, 'physical attempt lineage is immutable'); END";
const ATTEMPT_INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_insert_guard BEFORE INSERT ON physical_attempts WHEN NOT EXISTS (SELECT 1 FROM physical_attempt_lineages AS lineage JOIN capacity_reservations AS reservation ON reservation.id = NEW.reservation_id JOIN connection_intents AS intent ON intent.id = NEW.intent_id JOIN connection_request_authorizations AS witness ON witness.connection_id = NEW.connection_id AND witness.scale_set_id = NEW.scale_set_id AND witness.request_id = NEW.witness_request_id WHERE lineage.lineage_id = NEW.lineage_id AND lineage.connection_id = NEW.connection_id AND lineage.scale_set_id = NEW.scale_set_id AND lineage.binding_digest = NEW.binding_digest AND reservation.intent_id = NEW.intent_id AND reservation.connection_id = NEW.connection_id AND reservation.binding_digest = NEW.binding_digest AND reservation.engine_id = NEW.engine_id AND reservation.docker_root_digest = NEW.docker_root_digest AND intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id AND intent.scale_set_id = NEW.scale_set_id AND intent.kind = 'launch' AND witness.binding_digest = NEW.binding_digest AND witness.source_digest = NEW.source_digest AND witness.acquire_response_digest IS NEW.acquire_response_digest AND NEW.generation = COALESCE((SELECT MAX(prior.generation) + 1 FROM physical_attempts AS prior WHERE prior.lineage_id = NEW.lineage_id), 1) AND NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current_attempt WHERE current_attempt.lineage_id = NEW.lineage_id) AND (NEW.generation = 1 OR EXISTS (SELECT 1 FROM physical_attempt_outcomes AS outcome WHERE outcome.lineage_id = NEW.lineage_id AND outcome.generation = NEW.generation - 1))) BEGIN SELECT RAISE(ABORT, 'invalid physical attempt history'); END";
const OUTCOME_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_outcomes_no_update BEFORE UPDATE ON physical_attempt_outcomes BEGIN SELECT RAISE(ABORT, 'physical attempt outcome is immutable'); END";
const OUTCOME_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_outcomes_no_delete BEFORE DELETE ON physical_attempt_outcomes BEGIN SELECT RAISE(ABORT, 'physical attempt outcome is immutable'); END";
const CURRENT_INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_current_insert_guard BEFORE INSERT ON physical_attempt_current WHEN NOT EXISTS (SELECT 1 FROM physical_attempts AS attempt JOIN physical_attempt_lineages AS lineage ON lineage.lineage_id = attempt.lineage_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id JOIN connection_intents AS intent ON intent.id = attempt.intent_id JOIN connection_request_authorizations AS witness ON witness.connection_id = attempt.connection_id AND witness.scale_set_id = attempt.scale_set_id AND witness.request_id = attempt.witness_request_id WHERE attempt.lineage_id = NEW.lineage_id AND attempt.generation = NEW.generation AND attempt.reservation_id = NEW.reservation_id AND attempt.intent_id = NEW.intent_id AND lineage.connection_id = attempt.connection_id AND lineage.scale_set_id = attempt.scale_set_id AND lineage.binding_digest = attempt.binding_digest AND reservation.id = attempt.reservation_id AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND reservation.connection_id = attempt.connection_id AND reservation.binding_digest = attempt.binding_digest AND reservation.engine_id = attempt.engine_id AND reservation.docker_root_digest = attempt.docker_root_digest AND intent.id = attempt.intent_id AND intent.connection_id = attempt.connection_id AND intent.scale_set_id = attempt.scale_set_id AND intent.kind = 'launch' AND intent.state IN ('pending', 'uncertain', 'done') AND witness.binding_digest = attempt.binding_digest AND witness.source_digest = attempt.source_digest AND witness.acquire_response_digest IS attempt.acquire_response_digest) BEGIN SELECT RAISE(ABORT, 'invalid current physical attempt'); END";
const CURRENT_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_current_no_update BEFORE UPDATE ON physical_attempt_current BEGIN SELECT RAISE(ABORT, 'replace current physical attempt atomically'); END";
const CURRENT_DELETE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS physical_attempt_current_delete_guard BEFORE DELETE ON physical_attempt_current WHEN NOT EXISTS (SELECT 1 FROM physical_attempt_outcomes AS outcome JOIN capacity_reservations AS reservation ON reservation.id = outcome.reservation_id WHERE outcome.lineage_id = OLD.lineage_id AND outcome.generation = OLD.generation AND outcome.reservation_id = OLD.reservation_id AND reservation.state = 'released' AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1) BEGIN SELECT RAISE(ABORT, 'current attempt requires durable terminal release'); END";
const OUTCOME_INSERT_GUARD: &str = concat!(
    "CREATE TRIGGER IF NOT EXISTS physical_attempt_outcome_insert_guard BEFORE INSERT ON physical_attempt_outcomes WHEN NOT (",
    "EXISTS (SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE current.lineage_id = NEW.lineage_id AND current.generation = NEW.generation AND current.reservation_id = NEW.reservation_id AND NEW.outcome = 'acquire_no_effect' AND auth.acquire_state IN ('no_effect', 'rejected') AND auth.acquire_response_digest = NEW.proof_digest AND NEW.settlement_digest = NEW.proof_digest) OR ",
    "EXISTS (SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id JOIN connection_intents AS intent ON intent.id = current.intent_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = reservation.id JOIN connection_completion_cleanup AS completion ON completion.intent_id = intent.id AND completion.connection_id = intent.connection_id AND completion.scale_set_id = intent.scale_set_id JOIN connection_request_authorizations AS auth ON auth.connection_id = completion.connection_id AND auth.scale_set_id = completion.scale_set_id AND auth.request_id = completion.runner_request_id WHERE current.lineage_id = NEW.lineage_id AND current.generation = NEW.generation AND current.reservation_id = NEW.reservation_id AND NEW.outcome = 'normal_completion' AND reservation.state IN ('reserved', 'uncertain') AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND intent.cleanup_proven = 1 AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1 AND state.instance_binding_digest = cleanup.instance_binding_digest AND completion.runner_absent = 1 AND completion.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND auth.acquire_state = 'acquired' AND auth.completed_event_digest IS NOT NULL) OR ",
    "EXISTS (SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id JOIN connection_intents AS intent ON intent.id = current.intent_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id JOIN capacity_reservation_pre_jit_cleanup_claims AS claim ON claim.reservation_id = reservation.id JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_settlements AS settlement ON settlement.reservation_id = claim.reservation_id AND settlement.claim_generation = claim.claim_generation JOIN resource_probe_operations AS probe ON probe.operation_id = settlement.probe_operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 AND current_profile.generation_id = settlement.profile_generation_id JOIN host_storage_profiles AS profile ON profile.generation_id = current_profile.generation_id WHERE current.lineage_id = NEW.lineage_id AND current.generation = NEW.generation AND current.reservation_id = NEW.reservation_id AND NEW.outcome = 'pre_jit_capacity_available' AND NEW.proof_digest = settlement.sample_evidence_sha256 AND NEW.settlement_digest = settlement.sample_evidence_sha256 AND settlement.outcome = 'capacity_available' AND settlement.connection_id = attempt.connection_id AND settlement.operation_id = attempt.intent_id AND settlement.engine_id = attempt.engine_id AND settlement.docker_root_digest = attempt.docker_root_digest AND claim.connection_id = attempt.connection_id AND claim.operation_id = attempt.intent_id AND claim.engine_id = attempt.engine_id AND claim.docker_root_digest = attempt.docker_root_digest AND settlement.snapshot_sha256 = claim.snapshot_sha256 AND settlement.profile_digest = profile.profile_digest AND progress.phase = 'network_absent' AND claim.phase = 'claimed' AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND reservation.state = 'uncertain' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.state = 'uncertain' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND intent.github_runner_id IS NULL AND auth.acquire_state = 'acquired' AND NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = attempt.intent_id) AND NOT EXISTS (SELECT 1 FROM execution_publication_operations AS publication WHERE publication.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM connection_execution_proofs AS proof WHERE proof.reservation_id = reservation.id) AND EXISTS (SELECT 1 FROM capacity_reservation_pre_jit_create_responses AS response WHERE response.reservation_id = reservation.id AND response.role = 'runner' AND response.client_api_major = 1 AND response.client_api_minor = 53 AND response.response_status IN (400, 404, 409) AND response.engine_id = attempt.engine_id AND response.docker_root_digest = attempt.docker_root_digest) AND probe.probe_purpose = 'capacity' AND probe.phase = 'removed' AND probe.engine_id = attempt.engine_id AND probe.docker_root_digest = attempt.docker_root_digest AND lease.state = 'released' AND lease.engine_id = attempt.engine_id AND lease.docker_root_digest = attempt.docker_root_digest AND (SELECT COUNT(*) FROM capacity_reservation_pre_jit_cleanup_settlement_pools AS sampled WHERE sampled.reservation_id = reservation.id AND sampled.claim_generation = claim.claim_generation) = (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS reserved_pool WHERE reserved_pool.reservation_id = reservation.id AND reserved_pool.scope = 'shared-disk' AND reserved_pool.medium = 'disk')) ",
    ") BEGIN SELECT RAISE(ABORT, 'outcome does not retire current physical attempt'); END"
);
const REVISION_TABLES: [&str; 4] = [LINEAGE, ATTEMPT, CURRENT, OUTCOME];

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in [
        LINEAGE_DDL,
        ATTEMPT_DDL,
        CURRENT_DDL,
        OUTCOME_DDL,
        LINEAGE_NO_UPDATE,
        LINEAGE_NO_DELETE,
        ATTEMPT_NO_UPDATE,
        ATTEMPT_NO_DELETE,
        ATTEMPT_INSERT_GUARD,
        OUTCOME_NO_UPDATE,
        OUTCOME_NO_DELETE,
        CURRENT_INSERT_GUARD,
        CURRENT_NO_UPDATE,
        CURRENT_DELETE_GUARD,
        OUTCOME_INSERT_GUARD,
    ] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for table in REVISION_TABLES {
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let sql = format!(
                "CREATE TRIGGER IF NOT EXISTS {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
            );
            conn.execute(&sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

pub(super) async fn upgrade_from_v32(conn: &turso::Connection) -> Result<(), HostError> {
    bootstrap(conn).await?;
    let mut rows = conn
        .query(
            "SELECT link.connection_id, link.scale_set_id, link.request_id, link.reservation_id, link.physical_nonce, link.binding_digest, reservation.intent_id, reservation.engine_id, reservation.docker_root_digest, reservation.state, reservation.remote_cleanup_proven, reservation.local_cleanup_proven, intent.state, auth.source_digest, auth.acquire_response_digest FROM connection_request_reservations AS link JOIN capacity_reservations AS reservation ON reservation.id = link.reservation_id AND reservation.connection_id = link.connection_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN connection_request_authorizations AS auth ON auth.connection_id = link.connection_id AND auth.scale_set_id = link.scale_set_id AND auth.request_id = link.request_id AND auth.binding_digest = link.binding_digest ORDER BY link.reservation_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut legacy = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let state: String = row.get(9).map_err(|_| HostError::Journal)?;
        let remote_clean: i64 = row.get(10).map_err(|_| HostError::Journal)?;
        let local_clean: i64 = row.get(11).map_err(|_| HostError::Journal)?;
        let intent_state: String = row.get(12).map_err(|_| HostError::Journal)?;
        if (state == "released" && (remote_clean != 1 || local_clean != 1))
            || (state != "released"
                && !matches!(state.as_str(), "reserved" | "uncertain" | "recovering"))
            || !matches!(intent_state.as_str(), "pending" | "uncertain" | "done")
        {
            return Err(HostError::Journal);
        }
        legacy.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<i64>(1).map_err(|_| HostError::Journal)?,
            row.get::<i64>(2).map_err(|_| HostError::Journal)?,
            row.get::<i64>(3).map_err(|_| HostError::Journal)?,
            row.get::<String>(4).map_err(|_| HostError::Journal)?,
            row.get::<String>(5).map_err(|_| HostError::Journal)?,
            row.get::<i64>(6).map_err(|_| HostError::Journal)?,
            row.get::<String>(7).map_err(|_| HostError::Journal)?,
            row.get::<String>(8).map_err(|_| HostError::Journal)?,
            state == "released",
            row.get::<String>(13).map_err(|_| HostError::Journal)?,
            row.get::<Option<String>>(14)
                .map_err(|_| HostError::Journal)?,
        ));
    }
    drop(rows);
    for (
        connection_id,
        scale_set_id,
        request_id,
        reservation_id,
        nonce,
        binding,
        intent_id,
        engine_id,
        root_digest,
        released,
        source_digest,
        acquire_digest,
    ) in legacy
    {
        conn.execute(
            "INSERT INTO physical_attempt_lineages (connection_id, scale_set_id, binding_digest) VALUES (?1, ?2, ?3)",
            (connection_id.as_str(), scale_set_id, binding.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
        let lineage_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, resource_spec_digest, created_at) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, unixepoch())",
            (lineage_id, reservation_id, intent_id, nonce.as_str(), connection_id.as_str(), scale_set_id, request_id, binding.as_str(), source_digest.as_str(), acquire_digest, engine_id.as_str(), root_digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
        if !released {
            conn.execute(
                "INSERT INTO physical_attempt_current (lineage_id, generation, reservation_id, intent_id) VALUES (?1, 1, ?2, ?3)",
                (lineage_id, reservation_id, intent_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in objects() {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Err(HostError::Journal);
        };
        let actual: String = row.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || normalize_sql(&actual) != normalize_sql(expected)
        {
            return Err(HostError::Journal);
        }
    }
    validate_revision_triggers(conn).await?;
    validate_rows(conn).await
}

async fn validate_revision_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    for table in REVISION_TABLES {
        for (suffix, action) in [
            ("insert", "after insert"),
            ("update", "after update"),
            ("delete", "after delete"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let expected = normalize_sql(&format!(
                "CREATE TRIGGER {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
            ));
            let mut rows = conn
                .query(
                    "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                    (name.as_str(),),
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
            if normalize_sql(&actual) != expected
                || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

fn objects() -> [(&'static str, &'static str, &'static str); 15] {
    [
        ("table", LINEAGE, LINEAGE_DDL),
        ("table", ATTEMPT, ATTEMPT_DDL),
        ("table", CURRENT, CURRENT_DDL),
        ("table", OUTCOME, OUTCOME_DDL),
        (
            "trigger",
            "physical_attempt_lineages_no_update",
            LINEAGE_NO_UPDATE,
        ),
        (
            "trigger",
            "physical_attempt_lineages_no_delete",
            LINEAGE_NO_DELETE,
        ),
        ("trigger", "physical_attempts_no_update", ATTEMPT_NO_UPDATE),
        ("trigger", "physical_attempts_no_delete", ATTEMPT_NO_DELETE),
        (
            "trigger",
            "physical_attempt_insert_guard",
            ATTEMPT_INSERT_GUARD,
        ),
        (
            "trigger",
            "physical_attempt_outcomes_no_update",
            OUTCOME_NO_UPDATE,
        ),
        (
            "trigger",
            "physical_attempt_outcomes_no_delete",
            OUTCOME_NO_DELETE,
        ),
        (
            "trigger",
            "physical_attempt_current_insert_guard",
            CURRENT_INSERT_GUARD,
        ),
        (
            "trigger",
            "physical_attempt_current_no_update",
            CURRENT_NO_UPDATE,
        ),
        (
            "trigger",
            "physical_attempt_current_delete_guard",
            CURRENT_DELETE_GUARD,
        ),
        (
            "trigger",
            "physical_attempt_outcome_insert_guard",
            OUTCOME_INSERT_GUARD,
        ),
    ]
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_current AS current LEFT JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id LEFT JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id LEFT JOIN connection_intents AS intent ON intent.id = current.intent_id WHERE attempt.lineage_id IS NULL OR reservation.state NOT IN ('reserved', 'uncertain', 'recovering') OR intent.state NOT IN ('pending', 'uncertain', 'done') OR EXISTS (SELECT 1 FROM physical_attempt_outcomes AS outcome WHERE outcome.lineage_id = current.lineage_id AND outcome.generation = current.generation)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.to_ascii_lowercase()
        .replace(char::is_whitespace, "")
        .replace('"', "")
        .replace("ifnotexists", "")
}
