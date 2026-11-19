// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

fn definitions() -> [(&'static str, String); 4] {
    [
        ("image_imports_unresolved_insert", "image_imports", "INSERT", false),
        ("image_imports_unresolved_update", "image_imports", "UPDATE", true),
        ("image_import_registry_unresolved_insert", "image_import_registry", "INSERT", false),
        ("image_import_registry_unresolved_update", "image_import_registry", "UPDATE", true),
    ].map(|(name, table, event, update)| {
        let archive_exclusion = if update && table == "image_imports" { " AND id != OLD.id" } else { "" };
        let registry_exclusion = if update && table == "image_import_registry" { " AND id != OLD.id" } else { "" };
        (name, format!("CREATE TRIGGER {name} BEFORE {event} ON {table} WHEN NEW.state IN ('prepared', 'requested', 'uncertain') AND (EXISTS (SELECT 1 FROM image_imports WHERE engine_id = NEW.engine_id AND state IN ('prepared', 'requested', 'uncertain'){archive_exclusion}) OR EXISTS (SELECT 1 FROM image_import_registry WHERE engine_id = NEW.engine_id AND state IN ('prepared', 'requested', 'uncertain'){registry_exclusion})) BEGIN SELECT RAISE(ABORT, 'unresolved_import'); END"))
    })
}

pub(super) async fn install(conn: &turso::Connection) -> Result<(), HostError> {
    for (_, sql) in definitions() {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in definitions() {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                (name,),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let actual: String = row.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::sql_normalization::normalize(&actual)
                != super::sql_normalization::normalize(&expected)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
