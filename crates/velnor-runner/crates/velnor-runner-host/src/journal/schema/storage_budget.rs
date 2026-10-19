//! Immutable storage profiles and per-reservation storage evidence.

use std::collections::HashMap;

use crate::error::HostError;

use super::{ColumnInfo, read_table_columns};

const TABLES: [&str; 9] = [
    "host_storage_profiles",
    "host_storage_profile_publication",
    "host_storage_profile_pools",
    "host_storage_profile_roles",
    "host_storage_profile_current",
    "capacity_reservation_storage",
    "capacity_reservation_storage_roles",
    "capacity_reservation_storage_samples",
    "capacity_reservation_storage_pools",
];

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in statements() {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for statement in triggers() {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = expected_columns();
    let constraints = required_sql();
    for (index, table) in TABLES.iter().enumerate() {
        validate_table(conn, table, &expected[index], &constraints[index]).await?;
    }
    validate_triggers(conn).await?;
    crate::journal::connection::validate_storage_budget_schema(conn).await?;
    Ok(())
}

async fn validate_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = profile_trigger_contracts()
        .into_iter()
        .chain(reservation_trigger_contracts())
        .collect::<Vec<_>>();
    let mut rows = conn
        .query(
            "SELECT name, tbl_name, sql FROM sqlite_schema WHERE type = 'trigger'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = 0;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(0).map_err(|_| HostError::Journal)?;
        let table: String = row.get(1).map_err(|_| HostError::Journal)?;
        let Some((_, expected_table, before, action)) =
            expected.iter().find(|contract| contract.0 == name)
        else {
            if TABLES.contains(&table.as_str()) {
                return Err(HostError::Journal);
            }
            continue;
        };
        let sql = row
            .get::<String>(2)
            .map_err(|_| HostError::Journal)?
            .to_ascii_lowercase()
            .replace(char::is_whitespace, "");
        if table != *expected_table
            || !sql.contains(&before.to_ascii_lowercase().replace(char::is_whitespace, ""))
            || !sql.contains(&action.to_ascii_lowercase().replace(char::is_whitespace, ""))
        {
            return Err(HostError::Journal);
        }
        found += 1;
    }
    if found == expected.len() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn profile_trigger_contracts() -> [(&'static str, &'static str, &'static str, &'static str); 9] {
    [
        (
            "host_storage_profiles_no_update",
            "host_storage_profiles",
            "before update on host_storage_profiles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profiles_no_delete",
            "host_storage_profiles",
            "before delete on host_storage_profiles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_publication_no_update",
            "host_storage_profile_publication",
            "before update on host_storage_profile_publication",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_publication_no_delete",
            "host_storage_profile_publication",
            "before delete on host_storage_profile_publication",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_pools_no_update",
            "host_storage_profile_pools",
            "before update on host_storage_profile_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_pools_no_delete",
            "host_storage_profile_pools",
            "before delete on host_storage_profile_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_no_update",
            "host_storage_profile_roles",
            "before update on host_storage_profile_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_no_delete",
            "host_storage_profile_roles",
            "before delete on host_storage_profile_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_alias_target",
            "host_storage_profile_roles",
            "before insert on host_storage_profile_roles",
            "raise(abort,'invalid storage role alias')",
        ),
    ]
}

fn reservation_trigger_contracts() -> [(&'static str, &'static str, &'static str, &'static str); 9]
{
    [
        (
            "capacity_reservation_storage_no_update",
            "capacity_reservation_storage",
            "before update on capacity_reservation_storage",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_no_delete",
            "capacity_reservation_storage",
            "before delete on capacity_reservation_storage",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_roles_no_update",
            "capacity_reservation_storage_roles",
            "before update on capacity_reservation_storage_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_roles_no_delete",
            "capacity_reservation_storage_roles",
            "before delete on capacity_reservation_storage_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_samples_no_update",
            "capacity_reservation_storage_samples",
            "before update on capacity_reservation_storage_samples",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_samples_no_delete",
            "capacity_reservation_storage_samples",
            "before delete on capacity_reservation_storage_samples",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_pools_no_update",
            "capacity_reservation_storage_pools",
            "before update on capacity_reservation_storage_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_pools_no_delete",
            "capacity_reservation_storage_pools",
            "before delete on capacity_reservation_storage_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_current_no_delete",
            "host_storage_profile_current",
            "before delete on host_storage_profile_current",
            "raise(abort,'immutable storage evidence')",
        ),
    ]
}

fn triggers() -> [&'static str; 18] {
    [
        "CREATE TRIGGER IF NOT EXISTS host_storage_profiles_no_update BEFORE UPDATE ON host_storage_profiles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profiles_no_delete BEFORE DELETE ON host_storage_profiles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_publication_no_update BEFORE UPDATE ON host_storage_profile_publication BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_publication_no_delete BEFORE DELETE ON host_storage_profile_publication BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_pools_no_update BEFORE UPDATE ON host_storage_profile_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_pools_no_delete BEFORE DELETE ON host_storage_profile_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_no_update BEFORE UPDATE ON host_storage_profile_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_no_delete BEFORE DELETE ON host_storage_profile_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_alias_target BEFORE INSERT ON host_storage_profile_roles WHEN NEW.alias_of IS NOT NULL AND (NEW.role NOT IN ('cache-live', 'cache-stage') OR NEW.alias_of != 'runner-work' OR NOT EXISTS (SELECT 1 FROM host_storage_profile_roles target WHERE target.generation_id = NEW.generation_id AND target.role = NEW.alias_of AND target.alias_of IS NULL AND target.pool_id = NEW.pool_id AND target.medium = NEW.medium AND NEW.max_bytes <= target.max_bytes AND NEW.max_inodes <= target.max_inodes)) BEGIN SELECT RAISE(ABORT, 'invalid storage role alias'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_no_update BEFORE UPDATE ON capacity_reservation_storage BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_no_delete BEFORE DELETE ON capacity_reservation_storage BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_roles_no_update BEFORE UPDATE ON capacity_reservation_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_roles_no_delete BEFORE DELETE ON capacity_reservation_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_samples_no_update BEFORE UPDATE ON capacity_reservation_storage_samples BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_samples_no_delete BEFORE DELETE ON capacity_reservation_storage_samples BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_pools_no_update BEFORE UPDATE ON capacity_reservation_storage_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_pools_no_delete BEFORE DELETE ON capacity_reservation_storage_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_current_no_delete BEFORE DELETE ON host_storage_profile_current BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
    ]
}

async fn validate_table(
    conn: &turso::Connection,
    table: &str,
    expected: &[Shape],
    fragments: &[&str],
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, table).await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for shape in expected {
        if !column_matches(&columns, shape) {
            return Err(HostError::Journal);
        }
    }
    validate_sql(conn, table, fragments).await
}

fn column_matches(columns: &HashMap<String, ColumnInfo>, shape: &Shape) -> bool {
    columns.get(shape.name).is_some_and(|column| {
        column.declared_type.trim().eq_ignore_ascii_case(shape.kind)
            && column.not_null == shape.not_null
            && column.default_value.as_deref() == shape.default
            && column.primary_key_position == shape.primary_key
    })
}

async fn validate_sql(
    conn: &turso::Connection,
    table: &str,
    fragments: &[&str],
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?
        .to_ascii_lowercase()
        .replace(char::is_whitespace, "");
    if fragments.iter().any(|fragment| {
        !sql.contains(
            &fragment
                .to_ascii_lowercase()
                .replace(char::is_whitespace, ""),
        )
    }) {
        return Err(HostError::Journal);
    }
    Ok(())
}

mod contracts;
use contracts::{Shape, expected_columns, required_sql, statements};
