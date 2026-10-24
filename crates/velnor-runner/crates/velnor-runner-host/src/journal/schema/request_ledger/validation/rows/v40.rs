//! Current-version JIT row validation, including proof-gated null-ID retirement.

use crate::error::HostError;

pub(in crate::journal::schema::request_ledger::validation) async fn validate_jit_rows_v40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT j.intent_id, j.connection_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, i.assignment_key, i.github_runner_id, i.runner_name, i.jit_requested, i.cleanup_proven, r.acquire_state, j.witness_request_id, j.started_request_id, j.completed_request_id, (SELECT COUNT(*) FROM capacity_reservations AS capacity WHERE capacity.intent_id = j.intent_id AND capacity.connection_id = j.connection_id AND capacity.state = 'released' AND capacity.remote_cleanup_proven = 1 AND capacity.local_cleanup_proven = 1), (SELECT COUNT(*) FROM capacity_reservations AS capacity JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = capacity.id JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = capacity.id WHERE capacity.intent_id = j.intent_id AND capacity.connection_id = j.connection_id AND capacity.state != 'released' AND capacity.remote_cleanup_proven = 1 AND capacity.local_cleanup_proven = 1 AND state.storage_cleanup_proven = 1 AND cleanup.instance_binding_digest = state.instance_binding_digest), (SELECT COUNT(*) FROM physical_attempts AS attempt JOIN physical_attempt_jit_conflict_settlements AS settlement ON settlement.lineage_id = attempt.lineage_id AND settlement.generation = attempt.generation AND settlement.reservation_id = attempt.reservation_id WHERE attempt.intent_id = j.intent_id AND attempt.connection_id = j.connection_id) FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id JOIN connection_request_authorizations AS r ON r.connection_id = j.connection_id AND r.scale_set_id = j.scale_set_id AND r.request_id = j.witness_request_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        validate_jit_row(&row)?;
    }
    Ok(())
}

fn validate_jit_row(row: &turso::Row) -> Result<(), HostError> {
    let row = jit_validation_row(row)?;
    validate_jit_validation_row(&row)
}

fn validate_jit_validation_row(row: &JitValidationRow) -> Result<(), HostError> {
    let key_prefix = format!(
        "v1|{}|{}|remote|6:runner|",
        row.connection_id, row.scale_set_id
    );
    let null_id_retired = row.state == "conflict_retired"
        && row.runner_id.is_none()
        && row.runner_name.is_none()
        && row.intent_name.as_deref() == Some(row.requested.as_str())
        && row.stored_id.is_none()
        && row.cleanup_proven
        && row.released == 1
        && row.pending_cleanup == 0
        && row.settlements == 1;
    if row.intent_id <= 0
        || row.connection_id.is_empty()
        || row.scale_set_id <= 0
        || row.witness <= 0
        || row.requested.is_empty()
        || row.requested.len() > 128
        || !matches!(
            row.state.as_str(),
            "claimed"
                | "uncertain"
                | "bound"
                | "start_requested"
                | "start_uncertain"
                | "ready"
                | "started"
                | "completed"
                | "cleaned"
                | "conflict_retired"
        )
        || !row.jit_requested
        || row.witness_state != "acquired"
        || row.stored_witness != row.witness
        || !row.assignment_key.starts_with(&key_prefix)
        || row.runner_id.map(|value| value.to_string()) != row.stored_id
        || (row.runner_id.is_some() && row.runner_name != row.intent_name)
        || (row.runner_id.is_none()
            && row.state != "conflict_retired"
            && row.intent_name.as_deref() != Some(row.requested.as_str()))
        || row
            .runner_name
            .as_deref()
            .is_some_and(|value| value != row.requested)
        || (matches!(row.state.as_str(), "claimed" | "uncertain") && row.runner_id.is_some())
        || (matches!(
            row.state.as_str(),
            "bound"
                | "start_requested"
                | "start_uncertain"
                | "ready"
                | "started"
                | "completed"
                | "cleaned"
        ) && row.runner_id.is_none())
        || (row.state == "started" && row.started.is_none())
        || (row.state == "completed" && row.completed.is_none())
        || (row.state == "cleaned"
            && !((row.released == 1 && row.pending_cleanup == 0)
                || (row.released == 0 && row.pending_cleanup == 1)))
        || (row.cleanup_proven && row.completed.is_some() && row.state != "cleaned")
        || (row.state == "conflict_retired" && !null_id_retired)
        || (row.state == "conflict_retired" && (row.started.is_some() || row.completed.is_some()))
        || (matches!(
            row.state.as_str(),
            "claimed" | "uncertain" | "bound" | "start_requested" | "start_uncertain" | "ready"
        ) && (row.started.is_some() || row.completed.is_some()))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

struct JitValidationRow {
    intent_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness: i64,
    requested: String,
    state: String,
    runner_id: Option<i64>,
    runner_name: Option<String>,
    started: Option<i64>,
    completed: Option<i64>,
    assignment_key: String,
    stored_id: Option<String>,
    intent_name: Option<String>,
    jit_requested: bool,
    cleanup_proven: bool,
    witness_state: String,
    stored_witness: i64,
    released: i64,
    pending_cleanup: i64,
    settlements: i64,
}

fn jit_validation_row(row: &turso::Row) -> Result<JitValidationRow, HostError> {
    Ok(JitValidationRow {
        intent_id: row.get(0).map_err(|_| HostError::Journal)?,
        connection_id: row.get(1).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
        witness: row.get(3).map_err(|_| HostError::Journal)?,
        requested: row.get(4).map_err(|_| HostError::Journal)?,
        state: row.get(5).map_err(|_| HostError::Journal)?,
        runner_id: row.get(6).map_err(|_| HostError::Journal)?,
        runner_name: row.get(7).map_err(|_| HostError::Journal)?,
        started: row.get(8).map_err(|_| HostError::Journal)?,
        completed: row.get(9).map_err(|_| HostError::Journal)?,
        assignment_key: row.get(10).map_err(|_| HostError::Journal)?,
        stored_id: row.get(11).map_err(|_| HostError::Journal)?,
        intent_name: row.get(12).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(13).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(14).map_err(|_| HostError::Journal)?,
        witness_state: row.get(15).map_err(|_| HostError::Journal)?,
        stored_witness: row.get(16).map_err(|_| HostError::Journal)?,
        released: row.get(19).map_err(|_| HostError::Journal)?,
        pending_cleanup: row.get(20).map_err(|_| HostError::Journal)?,
        settlements: row.get(21).map_err(|_| HostError::Journal)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{JitValidationRow, validate_jit_validation_row};

    #[test]
    fn completed_jit_cannot_be_cleanup_proven_before_cleaned() {
        let mut row = completed_row();
        row.cleanup_proven = true;
        assert!(validate_jit_validation_row(&row).is_err());

        row.state = "cleaned".to_owned();
        row.released = 1;
        assert!(validate_jit_validation_row(&row).is_ok());
    }

    fn completed_row() -> JitValidationRow {
        JitValidationRow {
            intent_id: 1,
            connection_id: "connection".to_owned(),
            scale_set_id: 2,
            witness: 3,
            requested: "runner".to_owned(),
            state: "completed".to_owned(),
            runner_id: Some(4),
            runner_name: Some("runner".to_owned()),
            started: Some(5),
            completed: Some(6),
            assignment_key: "v1|connection|2|remote|6:runner|proof".to_owned(),
            stored_id: Some("4".to_owned()),
            intent_name: Some("runner".to_owned()),
            jit_requested: true,
            cleanup_proven: false,
            witness_state: "acquired".to_owned(),
            stored_witness: 3,
            released: 0,
            pending_cleanup: 0,
            settlements: 0,
        }
    }
}
