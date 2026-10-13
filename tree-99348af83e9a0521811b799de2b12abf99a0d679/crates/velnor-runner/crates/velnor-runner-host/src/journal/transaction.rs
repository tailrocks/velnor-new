//! Transactions for mutations keyed by durable row identity.

use std::ops::AsyncFnOnce;

use crate::error::HostError;

pub(super) async fn with_unique_id<T>(
    connection: &turso::Connection,
    id: i64,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = async {
        if row_count(connection, id).await? != 1 {
            return Err(HostError::Journal);
        }
        operation(connection).await
    }
    .await;
    match result {
        Ok(value) => {
            if connection.execute("COMMIT", ()).await.is_ok() {
                Ok(value)
            } else {
                connection
                    .execute("ROLLBACK", ())
                    .await
                    .map_err(|_| HostError::Journal)?;
                Err(HostError::Journal)
            }
        }
        Err(error) => {
            connection
                .execute("ROLLBACK", ())
                .await
                .map_err(|_| HostError::Journal)?;
            Err(error)
        }
    }
}

async fn row_count(connection: &turso::Connection, id: i64) -> Result<i64, HostError> {
    let mut rows = connection
        .query("SELECT COUNT(*) FROM intents WHERE id = ?1", [id])
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}
