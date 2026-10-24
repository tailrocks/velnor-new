use crate::error::HostError;
use crate::journal::Journal;

pub(super) async fn query_i64(
    journal: &Journal,
    sql: &str,
    reservation_id: i64,
) -> Result<i64, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
