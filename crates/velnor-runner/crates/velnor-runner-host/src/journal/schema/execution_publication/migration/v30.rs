//! This module adds the cleanup observer probe schema.

use crate::error::HostError;

use super::super::{CONTEXT_INSERT_GUARD, PROBE_PURPOSE_COLUMN, REQUEST_CONTEXT_COLUMN};

const LEGACY_PROBE_PURPOSE_COLUMN: &str = "probe_purpose TEXT NOT NULL DEFAULT 'capacity' CHECK (probe_purpose IN ('capacity', 'owned-cgroups', 'none'))";
const LEGACY_REQUEST_CONTEXT_COLUMN: &str = "request_context TEXT CHECK (request_context IS NULL OR length(CAST(request_context AS BLOB)) <= 8192)";

/// This migration lets cleanup observer requests use a 32,768-byte size limit.
pub(in crate::journal::schema) async fn upgrade_v30(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let current = resource_probe_operations_sql(connection).await?;
    if probe_columns_match(&current, PROBE_PURPOSE_COLUMN, REQUEST_CONTEXT_COLUMN) {
        return Ok(());
    }
    if !probe_columns_match(
        &current,
        LEGACY_PROBE_PURPOSE_COLUMN,
        LEGACY_REQUEST_CONTEXT_COLUMN,
    ) {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "DROP TRIGGER IF EXISTS execution_publication_context_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    replace_column(
        connection,
        "probe_purpose",
        "probe_purpose_v30",
        "probe_purpose_v30 TEXT NOT NULL DEFAULT 'capacity' CHECK (probe_purpose_v30 IN ('capacity', 'owned-cgroups', 'cleanup-observer', 'none'))",
    )
    .await?;
    replace_column(
        connection,
        "request_context",
        "request_context_v30",
        "request_context_v30 TEXT CHECK (request_context_v30 IS NULL OR ((probe_purpose = 'cleanup-observer' AND length(CAST(request_context_v30 AS BLOB)) <= 32768) OR (probe_purpose != 'cleanup-observer' AND length(CAST(request_context_v30 AS BLOB)) <= 8192)))",
    )
    .await?;
    connection
        .execute(CONTEXT_INSERT_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate_v30(connection).await
}

async fn resource_probe_operations_sql(
    connection: &turso::Connection,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'resource_probe_operations'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)
}

async fn replace_column(
    connection: &turso::Connection,
    previous: &str,
    replacement: &str,
    definition: &str,
) -> Result<(), HostError> {
    for statement in [
        format!("ALTER TABLE resource_probe_operations ADD COLUMN {definition}"),
        format!("UPDATE resource_probe_operations SET {replacement} = {previous}"),
        format!("ALTER TABLE resource_probe_operations DROP COLUMN {previous}"),
        format!("ALTER TABLE resource_probe_operations RENAME COLUMN {replacement} TO {previous}"),
    ] {
        connection
            .execute(&statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

/// This function makes sure that the cleanup request size limit is correct after migration.
pub(in crate::journal::schema) async fn validate_v30(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let sql = resource_probe_operations_sql(connection).await?;
    if !probe_columns_match(&sql, PROBE_PURPOSE_COLUMN, REQUEST_CONTEXT_COLUMN) {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn probe_columns_match(table_sql: &str, purpose: &str, context: &str) -> bool {
    column_definition(table_sql, "probe_purpose").is_some_and(|actual| {
        super::super::super::sql_normalization::normalize(actual)
            == super::super::super::sql_normalization::normalize(purpose)
    }) && column_definition(table_sql, "request_context").is_some_and(|actual| {
        super::super::super::sql_normalization::normalize(actual)
            == super::super::super::sql_normalization::normalize(context)
    })
}

fn column_definition<'a>(table_sql: &'a str, name: &str) -> Option<&'a str> {
    let opening = table_sql.find('(')?;
    let mut quote = None;
    let mut depth = 1_u32;
    let mut start = opening + 1;
    let bytes = table_sql.as_bytes();
    let mut index = opening + 1;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            let closing = if delimiter == b'[' { b']' } else { delimiter };
            if bytes[index] == closing {
                if delimiter != b'[' && bytes.get(index + 1) == Some(&closing) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`' | b'[') {
            quote = Some(bytes[index]);
        } else if bytes[index] == b'(' {
            depth += 1;
        } else if bytes[index] == b')' {
            depth -= 1;
            if depth == 0 {
                let candidate = table_sql[start..index].trim();
                if candidate
                    .split_whitespace()
                    .next()
                    .is_some_and(|column| column.eq_ignore_ascii_case(name))
                {
                    return Some(candidate);
                }
                return None;
            }
        } else if bytes[index] == b',' && depth == 1 {
            let candidate = table_sql[start..index].trim();
            if candidate
                .split_whitespace()
                .next()
                .is_some_and(|column| column.eq_ignore_ascii_case(name))
            {
                return Some(candidate);
            }
            start = index + 1;
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod cleanup_v30_tests {
    use super::{
        LEGACY_PROBE_PURPOSE_COLUMN, LEGACY_REQUEST_CONTEXT_COLUMN, PROBE_PURPOSE_COLUMN,
        REQUEST_CONTEXT_COLUMN, probe_columns_match,
    };

    #[test]
    fn cleanup_probe_column_contracts_match_exact_known_shapes() {
        let current = format!(
            "CREATE TABLE resource_probe_operations ({PROBE_PURPOSE_COLUMN}, {REQUEST_CONTEXT_COLUMN})"
        );
        assert!(probe_columns_match(
            &current,
            PROBE_PURPOSE_COLUMN,
            REQUEST_CONTEXT_COLUMN
        ));
        let weakened = current.replace("<= 32768", "<= 32768 OR 1");
        assert!(!probe_columns_match(
            &weakened,
            PROBE_PURPOSE_COLUMN,
            REQUEST_CONTEXT_COLUMN
        ));
        let legacy = format!(
            "CREATE TABLE resource_probe_operations ({LEGACY_PROBE_PURPOSE_COLUMN}, {LEGACY_REQUEST_CONTEXT_COLUMN})"
        );
        assert!(probe_columns_match(
            &legacy,
            LEGACY_PROBE_PURPOSE_COLUMN,
            LEGACY_REQUEST_CONTEXT_COLUMN
        ));
        let weakened_legacy = legacy.replace("<= 8192", "<= 8192 OR 1");
        assert!(!probe_columns_match(
            &weakened_legacy,
            LEGACY_PROBE_PURPOSE_COLUMN,
            LEGACY_REQUEST_CONTEXT_COLUMN
        ));
    }
}
