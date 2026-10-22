//! Startup reconciliation for durable Doctor tickets.

use crate::error::HostError;
use crate::journal::resource_probe::{ProbeLeaseState, ProbePhase, lease};

use super::DoctorProbeState;

pub(in crate::journal) async fn recover_at_startup(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT request.request_id, request.state, request.child_operation_id, probe.phase, lease.state FROM doctor_probe_requests AS request LEFT JOIN resource_probe_operations AS probe ON probe.operation_id = request.child_operation_id LEFT JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = request.child_operation_id WHERE request.state IN ('queued', 'running', 'recovering', 'uncertain') ORDER BY request.request_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut to_recover = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let state =
            DoctorProbeState::parse(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?;
        let operation_id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
        let phase: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
        let lease_state: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
        if state == DoctorProbeState::Queued {
            if operation_id.is_some() || phase.is_some() || lease_state.is_some() {
                return Err(HostError::Journal);
            }
            continue;
        }
        let operation_id = operation_id.ok_or(HostError::Journal)?;
        let phase = ProbePhase::parse(&phase.ok_or(HostError::Journal)?)?;
        let lease_state = ProbeLeaseState::parse(&lease_state.ok_or(HostError::Journal)?)?;
        if matches!(phase, ProbePhase::Removed | ProbePhase::Aborted)
            && lease_state == ProbeLeaseState::Released
        {
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET state = 'stale' WHERE request_id = ?1 AND state IN ('running', 'recovering', 'uncertain') AND child_operation_id = ?2",
                    (request_id, operation_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            continue;
        }
        if matches!(phase, ProbePhase::Removed | ProbePhase::Aborted)
            || lease_state == ProbeLeaseState::Released
        {
            return Err(HostError::Journal);
        }
        match lease_state {
            ProbeLeaseState::Reserved if phase == ProbePhase::Prepared => {}
            ProbeLeaseState::Active | ProbeLeaseState::Uncertain => {
                lease::mark_recovering(connection, &operation_id).await?;
            }
            ProbeLeaseState::Recovering => {}
            ProbeLeaseState::Reserved | ProbeLeaseState::Released => {
                return Err(HostError::Journal);
            }
        }
        to_recover.push((request_id, operation_id));
    }
    for (request_id, operation_id) in to_recover {
        let changed = connection
            .execute(
                "UPDATE doctor_probe_requests SET state = 'recovering' WHERE request_id = ?1 AND child_operation_id = ?2 AND state IN ('running', 'recovering', 'uncertain')",
                (request_id, operation_id.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
    }
    mark_completed_requests_stale(connection).await?;
    ensure_single_active_request(connection).await
}

async fn mark_completed_requests_stale(connection: &turso::Connection) -> Result<(), HostError> {
    let _completed_to_stale = connection
        .execute(
            "UPDATE doctor_probe_requests SET state = 'stale' WHERE state = 'completed'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn ensure_single_active_request(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM doctor_probe_requests WHERE state IN ('queued', 'running', 'recovering', 'uncertain')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let active = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if active > 1 {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}
