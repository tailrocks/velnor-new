//! Exact version-40 request-ledger validation.

use crate::error::HostError;

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_auth_table(conn).await?;
    super::validate_index_catalog(conn).await?;
    validate_jit_table(conn).await?;
    validate_jit_indexes(conn).await?;
    super::rows::validate_auth_rows(conn).await?;
    super::rows::validate_jit_rows_v40(conn).await
}

async fn validate_jit_table(conn: &turso::Connection) -> Result<(), HostError> {
    let actual = super::table_sql(conn, "connection_runner_jits").await?;
    let expected = super::super::super::jit_conflict_settlement::jit_table_sql();
    if super::super::super::sql_normalization::normalize(&actual)
        != super::super::super::sql_normalization::normalize(expected)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_jit_indexes(conn: &turso::Connection) -> Result<(), HostError> {
    for &(name, expected) in super::super::super::jit_conflict_settlement::jit_index_contracts() {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type='index' AND name=?1",
                (name,),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual: String = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::super::super::sql_normalization::normalize(&actual)
                != super::super::super::sql_normalization::normalize(expected)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
