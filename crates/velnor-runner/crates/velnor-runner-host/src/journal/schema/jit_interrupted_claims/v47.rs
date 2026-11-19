// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const STARTUP_INBOX_EXCLUSION: &str = "AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=attempt.connection_id AND inbox.scale_set_id=attempt.scale_set_id)\n";
const ACTIVE_POLICY_JOIN: &str = "JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest";

pub(super) fn startup_sql(sql: &str) -> String {
    sql.replace(STARTUP_INBOX_EXCLUSION, "")
}

pub(super) fn cleanup_sql(sql: &str) -> Result<String, HostError> {
    let mut sql = super::recovery_sql(sql);
    for (connection, snapshot) in [
        ("attempt", "receipt"),
        ("claim", "claim"),
        ("conflict", "conflict"),
    ] {
        for separator in [" ", "\n    "] {
            let old = format!(
                "JOIN connection_active_job_authority active ON active.connection_id={connection}.connection_id{separator}{ACTIVE_POLICY_JOIN}"
            );
            let new = format!(
                "JOIN connection_job_authority_policies policy ON policy.connection_id={snapshot}.connection_id AND policy.generation={snapshot}.authority_generation AND policy.policy_digest={snapshot}.authority_digest AND policy.binding_digest={snapshot}.binding_digest"
            );
            sql = sql.replace(&old, &new);
        }
    }
    if sql.contains("connection_active_job_authority active") {
        return Err(HostError::Journal);
    }
    Ok(sql
        .replace("active.generation", "policy.generation")
        .replace("active.policy_digest", "policy.policy_digest"))
}

pub(in crate::journal::schema) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, sql) in super::super::jit_conflict_settlement::schema_objects() {
        let current = super::recovery_sql(sql);
        let next = cleanup_sql(sql)?;
        if kind == "trigger" && current != next {
            replace_trigger(conn, name, &next).await?;
        }
    }
    for (kind, name, sql) in super::ddl::objects() {
        let next = startup_sql(&sql);
        if kind == "trigger" && sql != next {
            replace_trigger(conn, &name, &next).await?;
        }
    }
    conn.execute("PRAGMA user_version=47", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn replace_trigger(conn: &turso::Connection, name: &str, sql: &str) -> Result<(), HostError> {
    conn.execute(&format!("DROP TRIGGER {name}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
