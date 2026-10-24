use crate::error::HostError;

use super::{
    connection, ensure_completion_objects, ensure_legacy_columns, image_import, remote,
    scale_set_create,
};

pub(in crate::journal::schema) async fn from_version_zero(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    crate::journal_schema::bootstrap(conn).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    ensure_completion_objects(conn).await?;
    super::super::validate_current_schema(conn).await?;
    conn.execute(
        "UPDATE intents SET state = CASE WHEN state = 'failed' THEN 'uncertain' ELSE state END, cleanup_proven = 0 WHERE kind = 'launch' AND state IN ('failed', 'pending', 'uncertain')",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema) async fn from_version_one(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    crate::journal_schema::bootstrap(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    conn.execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema) async fn from_version_two(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    conn.execute("PRAGMA user_version = 3", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema) async fn from_version_three(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    super::super::validate_current_schema(conn).await?;
    conn.execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema) async fn from_version_four(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    image_import::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema) async fn from_version_five(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    scale_set_create::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
