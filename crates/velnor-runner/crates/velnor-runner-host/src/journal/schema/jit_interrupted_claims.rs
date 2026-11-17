// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

mod ddl;
mod v47;
pub(super) use v47::migrate as migrate_v46_to_v47;
#[cfg(test)]
mod migration_tests;

pub(super) fn recovery_sql(sql: &str) -> String {
    sql.replace(
        "physical_attempt_jit_conflict_receipts receipt",
        "physical_attempt_jit_recovery_sources receipt",
    )
    .replace(
        "receipt.response_status=409",
        "receipt.source_kind IN ('conflict','interrupted')",
    )
    .replace(
        "receipt.response_status IS NOT 409",
        "receipt.source_kind NOT IN ('conflict','interrupted')",
    )
}

pub(super) async fn versioned_recovery_sql(
    conn: &turso::Connection,
    sql: &str,
) -> Result<String, HostError> {
    let version = super::journal_version(conn).await?;
    if version >= 47 {
        v47::cleanup_sql(sql)
    } else if version >= 46 {
        Ok(recovery_sql(sql))
    } else {
        Ok(sql.to_owned())
    }
}

pub(super) async fn migrate_v45_to_v46(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v45(conn).await?;
    for (_, _, sql) in ddl::objects() {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (kind, name, sql) in super::jit_conflict_settlement::schema_objects() {
        if kind == "trigger" && sql.contains("physical_attempt_jit_conflict_receipts receipt") {
            conn.execute(&format!("DROP TRIGGER {name}"), ())
                .await
                .map_err(|_| HostError::Journal)?;
            conn.execute(&recovery_sql(sql), ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    conn.execute("PRAGMA user_version=46", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let version = super::journal_version(conn).await?;
    for (kind, name, expected) in ddl::objects() {
        let expected = if version >= 47 {
            v47::startup_sql(&expected)
        } else {
            expected
        };
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
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
    let mut rows = conn.query("SELECT COUNT(*) FROM physical_attempt_jit_interrupted_claims receipt LEFT JOIN physical_attempts attempt ON attempt.lineage_id=receipt.lineage_id AND attempt.generation=receipt.generation AND attempt.reservation_id=receipt.reservation_id AND attempt.intent_id=receipt.intent_id LEFT JOIN connection_request_authorizations auth ON auth.connection_id=receipt.connection_id AND auth.scale_set_id=receipt.scale_set_id AND auth.request_id=receipt.witness_request_id LEFT JOIN connection_job_authority_policies policy ON policy.connection_id=receipt.connection_id AND policy.generation=receipt.authority_generation AND policy.policy_digest=receipt.authority_digest LEFT JOIN connection_runner_jits jit ON jit.intent_id=receipt.intent_id AND jit.connection_id=receipt.connection_id WHERE attempt.lineage_id IS NULL OR attempt.connection_id IS NOT receipt.connection_id OR attempt.scale_set_id IS NOT receipt.scale_set_id OR attempt.witness_request_id IS NOT receipt.witness_request_id OR attempt.binding_digest IS NOT receipt.binding_digest OR attempt.source_digest IS NOT receipt.source_digest OR (attempt.acquire_response_digest IS NOT NULL AND attempt.acquire_response_digest IS NOT receipt.acquire_response_digest) OR attempt.engine_id IS NOT receipt.engine_id OR attempt.docker_root_digest IS NOT receipt.docker_root_digest OR auth.connection_id IS NULL OR auth.binding_digest IS NOT receipt.binding_digest OR auth.source_digest IS NOT receipt.source_digest OR auth.acquire_response_digest IS NOT receipt.acquire_response_digest OR auth.acquire_state IS NOT 'acquired' OR policy.connection_id IS NULL OR policy.binding_digest IS NOT receipt.binding_digest OR jit.intent_id IS NULL OR jit.scale_set_id IS NOT receipt.scale_set_id OR jit.witness_request_id IS NOT receipt.witness_request_id OR jit.requested_name IS NOT receipt.requested_name OR receipt.requested_name IS NOT ('v'||attempt.physical_nonce) OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_receipts conflict WHERE conflict.reservation_id=receipt.reservation_id)", ()).await.map_err(|_| HostError::Journal)?;
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

pub(in crate::journal) async fn recover_at_startup(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    conn.execute(
        &format!(
            "INSERT INTO physical_attempt_jit_interrupted_claims ({}) {}",
            ddl::COLUMNS,
            v47::startup_sql(ddl::ELIGIBLE)
        ),
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    for sql in [
        "UPDATE connection_runner_jits SET state='uncertain' WHERE state='claimed' AND EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.intent_id=connection_runner_jits.intent_id AND interrupted.connection_id=connection_runner_jits.connection_id AND interrupted.scale_set_id=connection_runner_jits.scale_set_id AND interrupted.witness_request_id=connection_runner_jits.witness_request_id AND interrupted.requested_name=connection_runner_jits.requested_name)",
        "UPDATE capacity_reservation_provisioning_state SET jit_state='uncertain' WHERE jit_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id=capacity_reservation_provisioning_state.reservation_id)",
        "UPDATE capacity_reservations SET state='uncertain' WHERE state='reserved' AND EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id=capacity_reservations.id)",
        "UPDATE connection_intents SET state='uncertain',launch_phase='jit_uncertain' WHERE state='pending' AND launch_phase='jit_requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.intent_id=connection_intents.id AND interrupted.connection_id=connection_intents.connection_id)",
    ] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
