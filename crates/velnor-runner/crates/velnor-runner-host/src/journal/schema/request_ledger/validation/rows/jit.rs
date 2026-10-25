use crate::error::HostError;

pub(in crate::journal::schema::request_ledger::validation) async fn validate_legacy_jit_rows(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_jit_rows_with_storage_cleanup(conn, "0").await
}

pub(in crate::journal::schema::request_ledger::validation) async fn validate_jit_rows(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_jit_rows_with_storage_cleanup(
        conn,
        "(SELECT COUNT(*) FROM capacity_reservations AS capacity JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = capacity.id JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = capacity.id WHERE capacity.intent_id = j.intent_id AND capacity.connection_id = j.connection_id AND capacity.state != 'released' AND capacity.remote_cleanup_proven = 1 AND capacity.local_cleanup_proven = 1 AND state.storage_cleanup_proven = 1 AND cleanup.instance_binding_digest = state.instance_binding_digest)",
    )
    .await
}

async fn validate_jit_rows_with_storage_cleanup(
    conn: &turso::Connection,
    pending_storage_cleanup_proofs: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            &format!("SELECT j.intent_id, j.connection_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, i.assignment_key, i.github_runner_id, i.runner_name, i.jit_requested, i.cleanup_proven, r.acquire_state, j.witness_request_id, j.started_request_id, j.completed_request_id, (SELECT COUNT(*) FROM capacity_reservations AS capacity WHERE capacity.intent_id = j.intent_id AND capacity.connection_id = j.connection_id AND capacity.state = 'released' AND capacity.remote_cleanup_proven = 1 AND capacity.local_cleanup_proven = 1), {pending_storage_cleanup_proofs} FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id JOIN connection_request_authorizations AS r ON r.connection_id = j.connection_id AND r.scale_set_id = j.scale_set_id AND r.request_id = j.witness_request_id"),
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
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let connection_id: String = row.get(1).map_err(|_| HostError::Journal)?;
    let set_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let witness: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let requested: String = row.get(4).map_err(|_| HostError::Journal)?;
    let state: String = row.get(5).map_err(|_| HostError::Journal)?;
    let runner_id: Option<i64> = row.get(6).map_err(|_| HostError::Journal)?;
    let runner_name: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    let assignment_key: String = row.get(10).map_err(|_| HostError::Journal)?;
    let stored_id: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
    let stored_name: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(13).map_err(|_| HostError::Journal)?;
    let cleaned: bool = row.get(14).map_err(|_| HostError::Journal)?;
    let witness_state: String = row.get(15).map_err(|_| HostError::Journal)?;
    let stored_witness: i64 = row.get(16).map_err(|_| HostError::Journal)?;
    let started_request: Option<i64> = row.get(17).map_err(|_| HostError::Journal)?;
    let completed_request: Option<i64> = row.get(18).map_err(|_| HostError::Journal)?;
    let released_cleanup_proofs: i64 = row.get(19).map_err(|_| HostError::Journal)?;
    let pending_storage_cleanup_proofs: i64 = row.get(20).map_err(|_| HostError::Journal)?;
    if id <= 0
        || connection_id.is_empty()
        || set_id <= 0
        || witness <= 0
        || requested.is_empty()
        || requested.len() > 128
        || !matches!(
            state.as_str(),
            "claimed"
                | "uncertain"
                | "bound"
                | "start_requested"
                | "start_uncertain"
                | "ready"
                | "started"
                | "completed"
                | "cleaned"
        )
        || !jit_requested
        || (cleaned && completed_request.is_some() && state != "cleaned")
        || witness_state != "acquired"
        || stored_witness != witness
        || !assignment_key.starts_with(&format!("v1|{connection_id}|{set_id}|remote|6:runner|"))
        || runner_id.map(|value| value.to_string()) != stored_id
        || (runner_id.is_some() && runner_name != stored_name)
        || (runner_id.is_none() && stored_name.as_deref() != Some(requested.as_str()))
        || runner_name
            .as_deref()
            .is_some_and(|value| value != requested)
        || (matches!(state.as_str(), "claimed" | "uncertain") && runner_id.is_some())
        || (matches!(
            state.as_str(),
            "bound"
                | "start_requested"
                | "start_uncertain"
                | "ready"
                | "started"
                | "completed"
                | "cleaned"
        ) && runner_id.is_none())
        || (state == "started" && started_request.is_none())
        || (state == "completed" && completed_request.is_none())
        || (state == "cleaned"
            && !((released_cleanup_proofs == 1 && pending_storage_cleanup_proofs == 0)
                || (released_cleanup_proofs == 0 && pending_storage_cleanup_proofs == 1)))
        || (matches!(
            state.as_str(),
            "claimed" | "uncertain" | "bound" | "start_requested" | "start_uncertain" | "ready"
        )) && (started_request.is_some() || completed_request.is_some())
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
