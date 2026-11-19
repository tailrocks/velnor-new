// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ABANDON_INTENTS, ABANDON_RESERVATIONS, unbound_pair};
use crate::error::HostError;

pub(super) async fn normalize_requested(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(crate::journal::unbound_ownership::CLASSIFICATION, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut attempts = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        attempts.push(row.get::<i64>(0).map_err(|_| HostError::Journal)?);
    }
    drop(rows);
    for reservation_id in attempts {
        for sql in [
            "UPDATE capacity_reservation_bootstrap_containers SET create_state='uncertain' WHERE reservation_id=?1 AND create_state='requested'",
            "UPDATE capacity_reservation_bootstrap_containers SET start_state='uncertain' WHERE reservation_id=?1 AND start_state='requested'",
            "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state='uncertain' WHERE reservation_id=?1 AND bootstrap_start_state='requested'",
            "UPDATE capacity_reservation_provisioning_objects SET create_state='uncertain' WHERE reservation_id=?1 AND create_state='requested'",
            "UPDATE capacity_reservation_networks SET create_state='uncertain' WHERE reservation_id=?1 AND create_state='requested'",
        ] {
            connection
                .execute(sql, [reservation_id])
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}
pub(super) async fn abandon_unbound_pairs(
    connection: &turso::Connection,
    reservation_id: Option<i64>,
    connection_id: Option<&str>,
) -> Result<(), HostError> {
    let sql = format!(
        "{} AND (?1 IS NULL OR r.id=?1) AND (?2 IS NULL OR r.connection_id=?2) AND {}",
        crate::journal::unbound_ownership::CLASSIFICATION,
        unbound_pair()
    );
    let mut rows = connection
        .query(&sql, (reservation_id, connection_id))
        .await
        .map_err(|_| HostError::Journal)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        ids.push(row.get::<i64>(0).map_err(|_| HostError::Journal)?);
    }
    drop(rows);
    for id in ids {
        for template in [ABANDON_RESERVATIONS, ABANDON_INTENTS] {
            connection
                .execute(
                    &template.replace("{pair}", &unbound_pair()),
                    (Some(id), connection_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}
