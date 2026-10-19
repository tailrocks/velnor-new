use super::sql_normalization::normalize;
pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    let transition_tables_were_absent = !transition_table_exists(
        connection,
        "capacity_reservation_bootstrap_containers",
    )
    .await?
        && !transition_table_exists(connection, "capacity_reservation_provisioning_state")
            .await?;
    for (_, statement) in TABLES.iter().chain(INDEXES.iter()) {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    reconcile_transition_guards(connection, transition_tables_were_absent).await?;
    for (_, statement) in TRIGGERS.iter().filter(|(name, _)| {
        !CLEANUP_GUARDS.iter().any(|(cleanup_name, _)| cleanup_name == name)
            && !is_transition_guard(name)
    }) {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    replace_cleanup_guards(connection).await?;
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in TABLES.iter().chain(INDEXES.iter()).chain(TRIGGERS.iter()) {
        let kind = object_kind(name);
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, *name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    if !current_transition_table_guard_set_matches(
        connection,
        CONTAINER_TRANSITION_TABLE,
        "provisioning_container_no_delete",
        "provisioning_container_transition",
    )
    .await?
        || !current_transition_table_guard_set_matches(
            connection,
            STATE_TRANSITION_TABLE,
            "provisioning_state_no_delete",
            "provisioning_state_transition",
        )
        .await?
    {
        return Err(HostError::Journal);
    }
    if scalar_count(
        connection,
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name = 'provisioning_storage_cleanup_guard'",
    )
    .await?
        != 0
    {
        return Err(HostError::Journal);
    }
    validate_rows(connection).await
}

fn object_kind(name: &str) -> &'static str {
    if TABLES.iter().any(|entry| entry.0 == name) {
        "table"
    } else if INDEXES.iter().any(|entry| entry.0 == name) {
        "index"
    } else {
        "trigger"
    }
}

include!("validation/queries.rs");

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    for queries in [
        INVALID_TOPOLOGY_QUERIES,
        INVALID_COUNTS_QUERIES,
        INVALID_CLEANUP_QUERIES,
        INVALID_METADATA_QUERIES,
    ] {
        for query in queries {
            ensure_zero(connection, query).await?;
        }
    }
    Ok(())
}

async fn ensure_zero(connection: &turso::Connection, sql: &str) -> Result<(), HostError> {
    if scalar_count(connection, sql).await? == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}
