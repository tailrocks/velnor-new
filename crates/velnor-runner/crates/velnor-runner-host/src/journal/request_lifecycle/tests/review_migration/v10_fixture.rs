use std::path::{Path, PathBuf};

use crate::error::HostError;

const VERSION_TEN_SCHEMA: &str = include_str!("v10_schema.sql");

pub(super) async fn make_version_ten_jit_schema(path: &Path) -> Result<PathBuf, HostError> {
    let target_path = sibling_path(path, "v10-target")?;
    let source_database = open_database(path).await?;
    let source = source_database.connect().map_err(|_| HostError::Journal)?;
    let target_database = open_database(&target_path).await?;
    let target = target_database.connect().map_err(|_| HostError::Journal)?;
    target
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .map_err(|_| HostError::Journal)?;
    create_version_ten_schema(&target).await?;
    copy_shared_rows(&source, &target).await?;
    verify_foreign_keys(&target).await?;
    target
        .execute("PRAGMA user_version = 10", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(source);
    drop(source_database);
    drop(target);
    drop(target_database);
    Ok(target_path)
}

async fn verify_foreign_keys(connection: &turso::Connection) -> Result<(), HostError> {
    let mut violations = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if violations
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> Result<PathBuf, HostError> {
    let name = path.file_name().ok_or(HostError::Journal)?;
    let mut result = path.to_path_buf();
    let mut renamed = name.to_os_string();
    renamed.push(format!(".{suffix}.db"));
    result.set_file_name(renamed);
    Ok(result)
}

async fn open_database(path: &Path) -> Result<turso::Database, HostError> {
    let path = path.to_str().ok_or(HostError::Journal)?;
    turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)
}

async fn create_version_ten_schema(connection: &turso::Connection) -> Result<(), HostError> {
    for statement in VERSION_TEN_SCHEMA
        .lines()
        .filter(|line| line.starts_with("CREATE "))
    {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn copy_shared_rows(
    source: &turso::Connection,
    target: &turso::Connection,
) -> Result<(), HostError> {
    let targets = table_names(target).await?;
    let sources = table_names(source).await?;
    for table in targets {
        if sources.contains(&table) {
            copy_table(source, target, &table).await?;
        }
    }
    Ok(())
}

async fn table_names(connection: &turso::Connection) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '__turso_internal_%' ORDER BY name",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut names = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        names.push(row.get::<String>(0).map_err(|_| HostError::Journal)?);
    }
    Ok(names)
}

async fn copy_table(
    source: &turso::Connection,
    target: &turso::Connection,
    table: &str,
) -> Result<(), HostError> {
    let source_columns = table_columns(source, table).await?;
    let target_columns = table_columns(target, table).await?;
    let columns = target_columns
        .into_iter()
        .filter(|column| source_columns.contains(column))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Ok(());
    }
    copy_rows(source, target, table, &columns).await
}

async fn table_columns(
    connection: &turso::Connection,
    table: &str,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(
            &format!("PRAGMA table_info(\"{}\")", quote_identifier(table)),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get::<String>(1).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}

async fn copy_rows(
    source: &turso::Connection,
    target: &turso::Connection,
    table: &str,
    columns: &[String],
) -> Result<(), HostError> {
    let names = columns
        .iter()
        .map(|column| format!("\"{}\"", quote_identifier(column)))
        .collect::<Vec<_>>();
    let quoted_table = format!("\"{}\"", quote_identifier(table));
    let select = format!("SELECT {} FROM {quoted_table}", names.join(", "));
    let placeholders = std::iter::repeat_n("?", columns.len())
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!(
        "INSERT INTO {quoted_table} ({}) VALUES ({placeholders})",
        names.join(", ")
    );
    let mut rows = source
        .query(&select, ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let values = (0..columns.len())
            .map(|index| row.get_value(index).map_err(|_| HostError::Journal))
            .collect::<Result<Vec<_>, _>>()?;
        target
            .execute(&insert, values)
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    value.replace('"', "\"\"")
}
