//! Checks of the retirement schema and existing records.

use crate::error::HostError;

use super::{ddl, guards};

mod acquire_proof;
mod pending_claims;

const INSERT_GUARDS: [&str; 3] = [
    guards::PROGRESS_INSERT_GUARD,
    guards::REMOTE_ABSENCE_INSERT_GUARD,
    guards::RETIREMENT_INSERT_GUARD,
];
const IMMUTABLE_GUARDS: [&str; 8] = [
    guards::CLAIMS_NO_UPDATE,
    guards::CLAIMS_NO_DELETE,
    guards::PROGRESS_NO_UPDATE,
    guards::PROGRESS_NO_DELETE,
    guards::REMOTE_ABSENCE_NO_UPDATE,
    guards::REMOTE_ABSENCE_NO_DELETE,
    guards::RETIREMENTS_NO_UPDATE,
    guards::RETIREMENTS_NO_DELETE,
];

pub(super) async fn validate_objects(
    conn: &turso::Connection,
    claim_guard: &str,
) -> Result<(), HostError> {
    validate_objects_with_view(conn, claim_guard, ddl::TERMINAL_PROOFS_VIEW).await
}

pub(super) async fn validate_objects_with_view(
    conn: &turso::Connection,
    claim_guard: &str,
    terminal_view: &str,
) -> Result<(), HostError> {
    let mut expected = Vec::new();
    expected.extend(
        ddl::TABLE_NAMES
            .into_iter()
            .zip([
                ddl::CLAIMS_DDL,
                ddl::PROGRESS_DDL,
                ddl::REMOTE_ABSENCE_DDL,
                ddl::RETIREMENTS_DDL,
            ])
            .map(|(name, sql)| ("table", name, sql)),
    );
    expected.push(("view", "physical_attempt_terminal_proofs", terminal_view));
    expected.push(("trigger", trigger_name(claim_guard), claim_guard));
    expected.extend(
        INSERT_GUARDS
            .into_iter()
            .map(|sql| ("trigger", trigger_name(sql), sql)),
    );
    expected.extend(
        IMMUTABLE_GUARDS
            .into_iter()
            .map(|sql| ("trigger", trigger_name(sql), sql)),
    );
    expected.push((
        "trigger",
        "physical_attempt_outcome_retirement_fence",
        guards::OUTCOME_RETIREMENT_FENCE,
    ));
    for (kind, name, expected_sql) in expected {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
                (kind, name),
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
        let actual_normalized = super::super::sql_normalization::normalize(&actual);
        let expected_normalized = super::super::sql_normalization::normalize(expected_sql);
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || actual_normalized != expected_normalized
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn trigger_name(sql: &str) -> &str {
    let body = sql.strip_prefix("CREATE TRIGGER ").unwrap_or(sql);
    body.split_ascii_whitespace().next().unwrap_or("")
}

pub(super) async fn validate_revision_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    for table in ddl::TABLE_NAMES {
        for (suffix, action) in [
            ("insert", "after insert"),
            ("update", "after update"),
            ("delete", "after delete"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let expected = super::super::sql_normalization::normalize(&format!(
                "CREATE TRIGGER {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision=revision+1 WHERE singleton=1; END"
            ));
            let mut rows = conn
                .query(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
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
            if super::super::sql_normalization::normalize(&actual) != expected
                || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

pub(super) async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    acquire_proof::validate_claim_responses(conn).await?;
    pending_claims::validate(conn).await?;
    let mut rows = conn
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims c LEFT JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id LEFT JOIN physical_attempt_current cur ON cur.lineage_id=c.lineage_id AND cur.generation=c.generation AND cur.reservation_id=c.reservation_id LEFT JOIN connection_request_authorizations auth ON auth.connection_id=c.connection_id AND auth.scale_set_id=c.scale_set_id AND auth.request_id=c.witness_request_id WHERE a.lineage_id IS NULL OR a.connection_id!=c.connection_id OR a.scale_set_id!=c.scale_set_id OR a.witness_request_id!=c.witness_request_id OR a.binding_digest!=c.binding_digest OR a.source_digest!=c.source_digest OR (a.acquire_response_digest IS NOT NULL AND a.acquire_response_digest!=c.acquire_response_digest) OR a.engine_id!=c.engine_id OR a.docker_root_digest!=c.docker_root_digest OR auth.connection_id IS NULL OR auth.binding_digest!=c.binding_digest OR auth.source_digest!=c.source_digest OR auth.acquire_state!='acquired' OR auth.acquire_response_json IS NULL OR auth.acquire_response_digest!=c.acquire_response_digest OR (cur.lineage_id IS NULL AND NOT EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirements r WHERE r.lineage_id=c.lineage_id AND r.generation=c.generation AND r.reservation_id=c.reservation_id))) + (SELECT COUNT(*) FROM (SELECT lineage_id,generation,reservation_id FROM physical_attempt_terminal_proofs GROUP BY lineage_id,generation,reservation_id HAVING COUNT(*)!=1)) + (SELECT COUNT(*) FROM physical_attempt_terminal_proofs p JOIN physical_attempt_current c ON c.lineage_id=p.lineage_id AND c.generation=p.generation AND c.reservation_id=p.reservation_id)",
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
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    validate_retirement_digests(conn).await?;
    Ok(())
}

async fn validate_retirement_digests(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT lineage_id,generation,reservation_id,claim_generation,metadata_generation,cleanup_probe_operation_id,proof_digest,settlement_digest FROM physical_attempt_post_jit_retirements ORDER BY lineage_id,generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let lineage_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let generation = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
        let reservation_id = row.get::<i64>(2).map_err(|_| HostError::Journal)?;
        let claim_generation = row.get::<i64>(3).map_err(|_| HostError::Journal)?;
        let metadata_generation = row.get::<i64>(4).map_err(|_| HostError::Journal)?;
        let probe_operation_id = row.get::<String>(5).map_err(|_| HostError::Journal)?;
        let proof_digest = row.get::<String>(6).map_err(|_| HostError::Journal)?;
        let settlement_digest = row.get::<String>(7).map_err(|_| HostError::Journal)?;
        if !crate::journal::connection::validate_retirement_digest(
            conn,
            crate::journal::connection::RetirementDigestIdentity {
                lineage_id,
                generation,
                reservation_id,
                claim_generation,
                metadata_generation,
                cleanup_probe_operation_id: &probe_operation_id,
            },
            &proof_digest,
            &settlement_digest,
        )
        .await?
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
