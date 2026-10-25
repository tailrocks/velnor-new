use super::super::sql_normalization::normalize;
use super::{
    CLAIM_INSERT_GUARD, CLAIM_NO_DELETE, CLAIM_UPDATE_GUARD, CLAIMS, CREATE_SPEC_INSERT_GUARD,
    CREATE_SPEC_NO_DELETE, CREATE_SPEC_NO_UPDATE, CREATE_SPECS, HostError, INSERT_GUARD, NO_DELETE,
    NO_UPDATE, PROBE_INSERT_GUARD, PROBE_NO_DELETE, PROBE_NO_UPDATE, PROBES, PROGRESS,
    PROGRESS_INSERT_GUARD, PROGRESS_NO_DELETE, PROGRESS_UPDATE_GUARD, SETTLEMENT,
    SETTLEMENT_INSERT_GUARD, SETTLEMENT_NO_DELETE, SETTLEMENT_NO_UPDATE, SETTLEMENT_POOL,
    SETTLEMENT_POOL_INSERT_GUARD, SETTLEMENT_POOL_NO_DELETE, SETTLEMENT_POOL_NO_UPDATE, TABLE,
};

pub(in crate::journal::schema) async fn validate(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate_receipts_and_claims(connection).await?;
    validate_probe_and_settlement(connection).await
}

async fn validate_receipts_and_claims(connection: &turso::Connection) -> Result<(), HostError> {
    if object_sql(
        connection,
        "table",
        "capacity_reservation_pre_jit_create_responses",
    )
    .await?
    .as_deref()
    .is_none_or(|actual| normalize(actual) != normalize(TABLE))
    {
        return Err(HostError::Journal);
    }
    if object_sql(
        connection,
        "table",
        "capacity_reservation_pre_jit_create_specs",
    )
    .await?
    .as_deref()
    .is_none_or(|actual| normalize(actual) != normalize(CREATE_SPECS))
    {
        return Err(HostError::Journal);
    }
    for (name, expected) in [
        ("pre_jit_create_response_insert_guard", INSERT_GUARD),
        ("pre_jit_create_response_no_update", NO_UPDATE),
        ("pre_jit_create_response_no_delete", NO_DELETE),
        ("pre_jit_create_spec_insert_guard", CREATE_SPEC_INSERT_GUARD),
        ("pre_jit_create_spec_no_update", CREATE_SPEC_NO_UPDATE),
        ("pre_jit_create_spec_no_delete", CREATE_SPEC_NO_DELETE),
        ("pre_jit_cleanup_claim_insert_guard", CLAIM_INSERT_GUARD),
        ("pre_jit_cleanup_claim_update_guard", CLAIM_UPDATE_GUARD),
        ("pre_jit_cleanup_claim_no_delete", CLAIM_NO_DELETE),
        (
            "pre_jit_cleanup_progress_insert_guard",
            PROGRESS_INSERT_GUARD,
        ),
        (
            "pre_jit_cleanup_progress_update_guard",
            PROGRESS_UPDATE_GUARD,
        ),
        ("pre_jit_cleanup_progress_no_delete", PROGRESS_NO_DELETE),
    ] {
        let Some(actual) = object_sql(connection, "trigger", name).await? else {
            return Err(HostError::Journal);
        };
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    if object_sql(
        connection,
        "table",
        "capacity_reservation_pre_jit_cleanup_claims",
    )
    .await?
    .as_deref()
    .is_none_or(|actual| normalize(actual) != normalize(CLAIMS))
    {
        return Err(HostError::Journal);
    }
    if object_sql(
        connection,
        "table",
        "capacity_reservation_pre_jit_cleanup_progress",
    )
    .await?
    .as_deref()
    .is_none_or(|actual| normalize(actual) != normalize(PROGRESS))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_probe_and_settlement(connection: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in [
        (
            "table",
            "capacity_reservation_pre_jit_cleanup_probes",
            PROBES,
        ),
        (
            "trigger",
            "pre_jit_cleanup_probe_insert_guard",
            PROBE_INSERT_GUARD,
        ),
        (
            "trigger",
            "pre_jit_cleanup_probe_no_update",
            PROBE_NO_UPDATE,
        ),
        (
            "trigger",
            "pre_jit_cleanup_probe_no_delete",
            PROBE_NO_DELETE,
        ),
        (
            "table",
            "capacity_reservation_pre_jit_cleanup_settlements",
            SETTLEMENT,
        ),
        (
            "table",
            "capacity_reservation_pre_jit_cleanup_settlement_pools",
            SETTLEMENT_POOL,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_insert_guard",
            SETTLEMENT_INSERT_GUARD,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_pool_insert_guard",
            SETTLEMENT_POOL_INSERT_GUARD,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_no_update",
            SETTLEMENT_NO_UPDATE,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_no_delete",
            SETTLEMENT_NO_DELETE,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_pool_no_update",
            SETTLEMENT_POOL_NO_UPDATE,
        ),
        (
            "trigger",
            "pre_jit_cleanup_settlement_pool_no_delete",
            SETTLEMENT_POOL_NO_DELETE,
        ),
    ] {
        let Some(actual) = object_sql(connection, kind, name).await? else {
            return Err(HostError::Journal);
        };
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn object_sql(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get(0).map(Some).map_err(|_| HostError::Journal)
}
