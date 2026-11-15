// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

use super::ReservationRow;

const JOURNAL_FILE: &str = "launch.db";

const QUERY: &str = "SELECT reservation.id, reservation.connection_id, reservation.state, intent.state, init.create_dispatch_state, init.create_state, init.start_state, init.abort_state, init.helper_id, init.abort_helper_id FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id LEFT JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = reservation.id WHERE (?1 IS NULL OR reservation.id = ?1) ORDER BY reservation.id";

pub fn show_reservations(
    state: &Path,
    reservation: Option<i64>,
) -> Result<Vec<ReservationRow>, HostError> {
    let path = state.join(JOURNAL_FILE);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| HostError::Path)?;
    if !metadata.file_type().is_file() {
        return Err(HostError::Path);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(read_rows(&path, reservation))
}

async fn read_rows(
    path: &Path,
    reservation: Option<i64>,
) -> Result<Vec<ReservationRow>, HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
        .read_only(true)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut query = connection
        .query(QUERY, [reservation])
        .await
        .map_err(|_| HostError::Journal)?;
    let mut rows = Vec::new();
    while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
        rows.push(ReservationRow {
            reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
            connection_id: row.get(1).map_err(|_| HostError::Journal)?,
            reservation_state: row.get(2).map_err(|_| HostError::Journal)?,
            intent_state: row.get(3).map_err(|_| HostError::Journal)?,
            create_dispatch_state: row.get(4).map_err(|_| HostError::Journal)?,
            create_state: row.get(5).map_err(|_| HostError::Journal)?,
            start_state: row.get(6).map_err(|_| HostError::Journal)?,
            abort_state: row.get(7).map_err(|_| HostError::Journal)?,
            helper_id: row.get(8).map_err(|_| HostError::Journal)?,
            abort_helper_id: row.get(9).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(rows)
}
