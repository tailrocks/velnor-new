// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const VERSION: i64 = 61;

const WINDOWS_AND_EXACT_PREDECESSORS: [(&str, &str); 4] = [
    (
        "unixepoch()-NEW.created_at IN (0,1)",
        "NEW.created_at=unixepoch()",
    ),
    (
        "NEW.commit_observed_at-unixepoch() NOT IN (0,1)",
        "NEW.commit_observed_at!=unixepoch()+1",
    ),
    ("NEW.claimed_at<unixepoch()", "NEW.claimed_at<unixepoch()+1"),
    (
        "candidate.created_at-NEW.created_at IN (0,1)",
        "candidate.created_at=NEW.created_at",
    ),
];

pub(super) fn matches(
    version: i64,
    actual: &str,
    expected: &str,
    normalize: fn(&str) -> String,
) -> bool {
    let actual = normalize(actual);
    actual == normalize(expected) || (version < VERSION && actual == normalize(&exact(expected)))
}

pub(super) async fn migrate_v60_to_v61(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema(conn).await?;
    for (name, sql) in guards(conn).await? {
        if exact(&sql) == sql {
            return Err(HostError::Journal);
        }
        recreate(conn, &name, &sql).await?;
    }
    conn.execute(&format!("PRAGMA user_version = {VERSION}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn restore_exact_guards_at_v60(conn: &turso::Connection) -> Result<(), HostError> {
    for (name, sql) in guards(conn).await? {
        recreate(conn, &name, &exact(&sql)).await?;
    }
    conn.execute("PRAGMA user_version = 60", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn guards(conn: &turso::Connection) -> Result<Vec<(String, String)>, HostError> {
    let mut guards = super::jit_claim_deadline::clock_guards();
    guards.push(super::unbound_retirement::clock_guard()?);
    guards.push(super::jit_conflict_recovery::clock_guard());
    guards.extend(super::jit_conflict_settlement::clock_guards(conn).await?);
    guards.push(super::jit_interrupted_claims::clock_guard()?);
    guards.extend(super::post_jit_retirement::clock_guards()?);
    Ok(guards)
}

async fn recreate(conn: &turso::Connection, name: &str, sql: &str) -> Result<(), HostError> {
    conn.execute(&format!("DROP TRIGGER {name}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn exact(sql: &str) -> String {
    WINDOWS_AND_EXACT_PREDECESSORS
        .iter()
        .fold(sql.to_owned(), |sql, (window, exact)| {
            sql.replace(window, exact)
        })
}

#[cfg(test)]
mod tests;
