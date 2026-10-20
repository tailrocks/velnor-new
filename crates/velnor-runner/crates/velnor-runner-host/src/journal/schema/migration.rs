//! Versioned journal migrations.

use crate::error::HostError;

use super::{
    connection, doctor_probe, image_import, provisioning, remote, request_ledger,
    request_reservations, scale_set_create, session, session_refresh, source_authorization,
    storage_budget, storage_initialization, storage_network,
};

#[path = "migration/completion.rs"]
mod completion;

pub(super) use completion::{ensure_completion_objects, ensure_legacy_columns};

pub(super) async fn from_version_zero(conn: &turso::Connection) -> Result<(), HostError> {
    crate::journal_schema::bootstrap(conn).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    ensure_completion_objects(conn).await?;
    super::validate_current_schema(conn).await?;
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

pub(super) async fn from_version_one(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    crate::journal_schema::bootstrap(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    conn.execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_two(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    conn.execute("PRAGMA user_version = 3", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_three(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    super::validate_current_schema(conn).await?;
    conn.execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_four(conn: &turso::Connection) -> Result<(), HostError> {
    image_import::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_five(conn: &turso::Connection) -> Result<(), HostError> {
    scale_set_create::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_six(conn: &turso::Connection) -> Result<(), HostError> {
    image_import::upgrade_identity(conn).await?;
    conn.execute("PRAGMA user_version = 7", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_seven(conn: &turso::Connection) -> Result<(), HostError> {
    source_authorization::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_eight(conn: &turso::Connection) -> Result<(), HostError> {
    source_authorization::upgrade_v8(conn).await?;
    conn.execute("PRAGMA user_version = 9", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_nine(conn: &turso::Connection) -> Result<(), HostError> {
    request_ledger::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_ten(conn: &turso::Connection) -> Result<(), HostError> {
    request_ledger::upgrade_cleaned_runner_state(conn).await?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_eleven(conn: &turso::Connection) -> Result<(), HostError> {
    connection::upgrade_capacity_policy(conn).await?;
    crate::journal_schema::upgrade_probe_lease(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    connection::validate_connection_schema(conn).await?;
    crate::journal_schema::validate_probe_lease(conn).await?;
    conn.execute("PRAGMA user_version = 12", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twelve(conn: &turso::Connection) -> Result<(), HostError> {
    crate::journal::capacity_header::bootstrap_schema_connection(conn).await?;
    session::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 13", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirteen(conn: &turso::Connection) -> Result<(), HostError> {
    session_refresh::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 14", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_fourteen(conn: &turso::Connection) -> Result<(), HostError> {
    storage_budget::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 15", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_fifteen(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 16", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_sixteen(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::upgrade_transition_guards(conn).await?;
    provisioning::upgrade_from_version_sixteen(conn).await?;
    conn.execute("PRAGMA user_version = 17", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v17(conn).await?;
    Ok(())
}

pub(super) async fn from_version_seventeen(conn: &turso::Connection) -> Result<(), HostError> {
    request_reservations::bootstrap(conn).await?;
    doctor_probe::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 18", ())
        .await
        .map_err(|_| HostError::Journal)?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    crate::journal_schema::install_v18_revision_triggers(conn).await?;
    Ok(())
}

pub(super) async fn from_version_eighteen(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::upgrade_transition_guards(conn).await?;
    provisioning::upgrade_from_version_eighteen(conn).await?;
    conn.execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    crate::journal_schema::install_v18_revision_triggers(conn).await?;
    super::validate_persisted_schema_v19(conn).await?;
    Ok(())
}

pub(super) async fn from_version_nineteen(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::upgrade_transition_guards(conn).await?;
    provisioning::replace_cleanup_guards(conn).await?;
    storage_initialization::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 20", ())
        .await
        .map_err(|_| HostError::Journal)?;
    crate::journal_schema::install_v20_revision_triggers(conn).await?;
    super::validate_persisted_schema_v20(conn).await?;
    Ok(())
}

pub(super) async fn from_version_twenty(conn: &turso::Connection) -> Result<(), HostError> {
    storage_network::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 21", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_one(conn: &turso::Connection) -> Result<(), HostError> {
    storage_initialization::upgrade_v22(conn).await?;
    conn.execute("PRAGMA user_version = 22", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_two(conn: &turso::Connection) -> Result<(), HostError> {
    super::storage_root_backing::bootstrap_v30(conn).await?;
    conn.execute("PRAGMA user_version = 23", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_three(conn: &turso::Connection) -> Result<(), HostError> {
    super::execution_authorization::bootstrap(conn).await?;
    // Existing v23–v27 journals still have the original large receipt CHECKs.
    // Rebuild them before the strict current-schema validator compares DDL.
    super::execution_authorization::upgrade_v28(conn).await?;
    super::execution_authorization::validate(conn).await?;
    super::execution_publication::bootstrap(conn).await?;
    super::execution_publication::validate(conn).await?;
    conn.execute("PRAGMA user_version = 24", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_four(conn: &turso::Connection) -> Result<(), HostError> {
    super::execution_publication::upgrade_v25(conn).await?;
    conn.execute("PRAGMA user_version = 25", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_five(conn: &turso::Connection) -> Result<(), HostError> {
    super::storage_handoff::bootstrap(conn).await?;
    super::storage_handoff::validate(conn).await?;
    conn.execute("PRAGMA user_version = 26", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_six(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::upgrade_transition_guards(conn).await?;
    storage_initialization::repair_v27_metadata_volume_name_guard(conn).await?;
    super::storage_root_backing::bootstrap_v30(conn).await?;
    super::storage_root_backing::upgrade_v27(conn).await?;
    super::execution_publication::upgrade_v27(conn).await?;
    super::execution_authorization::upgrade_v28(conn).await?;
    conn.execute("PRAGMA user_version = 27", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v27(conn).await
}

pub(super) async fn from_version_twenty_seven(conn: &turso::Connection) -> Result<(), HostError> {
    super::execution_authorization::upgrade_v28(conn).await?;
    conn.execute("PRAGMA user_version = 28", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_eight(conn: &turso::Connection) -> Result<(), HostError> {
    provisioning::upgrade_transition_guards(conn).await?;
    conn.execute("PRAGMA user_version = 29", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_twenty_nine(conn: &turso::Connection) -> Result<(), HostError> {
    super::storage_root_backing::upgrade_v30(conn).await?;
    super::execution_publication::upgrade_v30(conn).await?;
    super::cleanup_observer_witness::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 30", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirty(conn: &turso::Connection) -> Result<(), HostError> {
    super::storage_root_backing::upgrade_v31(conn).await?;
    conn.execute("PRAGMA user_version = 31", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirty_one(conn: &turso::Connection) -> Result<(), HostError> {
    super::cleanup_observer_witness::upgrade_for_v32(conn).await?;
    super::pre_jit_cleanup::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 32", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirty_two(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v32(conn).await?;
    super::physical_attempts::upgrade_from_v32(conn).await?;
    super::storage_network::upgrade_for_v33(conn).await?;
    conn.execute("PRAGMA user_version = 33", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirty_three(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v33(conn).await?;
    super::physical_attempts::upgrade_from_v33(conn).await?;
    conn.execute("PRAGMA user_version = 34", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_thirty_four(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v34(conn).await?;
    super::post_jit_retirement::bootstrap(conn).await?;
    super::physical_attempts::upgrade_from_v34(conn).await?;
    conn.execute("PRAGMA user_version = 35", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
