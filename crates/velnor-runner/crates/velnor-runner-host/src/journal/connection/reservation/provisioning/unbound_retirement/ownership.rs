// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(super) use crate::journal::unbound_ownership::base_sql;

pub(in crate::journal::connection::reservation::provisioning) async fn owns_cleanup(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let base = base_sql();
    eligible(connection, connection_id, reservation_id, &base).await
}

pub(in crate::journal::connection::reservation::provisioning) async fn reserves_cleanup(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<bool, HostError> {
    eligible(
        connection,
        connection_id,
        reservation_id,
        crate::journal::unbound_ownership::CLASSIFICATION,
    )
    .await
}

async fn eligible(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    base: &str,
) -> Result<bool, HostError> {
    let sql = format!(
        "{base} AND r.id=?1 AND r.connection_id=?2 AND r.state='uncertain' AND i.state='uncertain' AND {} AND NOT EXISTS(SELECT 1 FROM capacity_reservation_provisioning_objects object WHERE object.reservation_id=r.id AND object.create_state='requested')",
        super::unbound_pair()
    );
    let mut rows = connection
        .query(&sql, (reservation_id, connection_id))
        .await
        .map_err(|_| HostError::Journal)?;
    let found = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(found)
}
