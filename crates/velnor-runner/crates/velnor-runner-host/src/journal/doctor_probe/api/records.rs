//! Doctor probe records.

use crate::error::HostError;
use crate::journal::resource_probe::{ProbePhase, ProbeRow, ProbeSeed, operations};

use super::{DoctorProbeRequestRecord, DoctorProbeState};

pub(in crate::journal::doctor_probe) struct TicketRow {
    pub(super) request_id: i64,
    pub(super) nonce: String,
    pub(super) instance_id: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) state: DoctorProbeState,
    pub(super) child_operation_id: Option<String>,
}

pub(in crate::journal::doctor_probe) async fn ticket_by_id(
    connection: &turso::Connection,
    request_id: i64,
) -> Result<TicketRow, HostError> {
    let mut rows = connection
        .query(
            "SELECT request_id, nonce, instance_id, engine_id, docker_root_digest, state, child_operation_id FROM doctor_probe_requests WHERE request_id = ?1",
            [request_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    decode_ticket(&row)
}

pub(in crate::journal::doctor_probe) async fn ticket_by_nonce(
    connection: &turso::Connection,
    nonce: &str,
) -> Result<Option<TicketRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT request_id, nonce, instance_id, engine_id, docker_root_digest, state, child_operation_id FROM doctor_probe_requests WHERE nonce = ?1",
            [nonce],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let ticket = decode_ticket(&row)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(ticket))
}

pub(in crate::journal::doctor_probe) fn decode_ticket(
    row: &turso::Row,
) -> Result<TicketRow, HostError> {
    Ok(TicketRow {
        request_id: row.get(0).map_err(|_| HostError::Journal)?,
        nonce: row.get(1).map_err(|_| HostError::Journal)?,
        instance_id: row.get(2).map_err(|_| HostError::Journal)?,
        engine_id: row.get(3).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(4).map_err(|_| HostError::Journal)?,
        state: DoctorProbeState::parse(&row.get::<String>(5).map_err(|_| HostError::Journal)?)?,
        child_operation_id: row.get(6).map_err(|_| HostError::Journal)?,
    })
}

pub(in crate::journal::doctor_probe) async fn active_request_id(
    connection: &turso::Connection,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT request_id FROM doctor_probe_requests WHERE state IN ('queued', 'running', 'recovering', 'uncertain') ORDER BY request_id LIMIT 2",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let request_id = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(request_id))
}

pub(in crate::journal::doctor_probe) async fn record(
    connection: &turso::Connection,
    ticket: TicketRow,
) -> Result<DoctorProbeRequestRecord, HostError> {
    let child = match ticket.child_operation_id.as_deref() {
        Some(operation_id) => Some(probe_by_id(connection, operation_id).await?),
        None => None,
    };
    Ok(DoctorProbeRequestRecord {
        request_id: ticket.request_id,
        nonce: ticket.nonce,
        engine_id: ticket.engine_id,
        docker_root_digest: ticket.docker_root_digest,
        state: ticket.state,
        child,
    })
}

pub(in crate::journal::doctor_probe) async fn probe_by_id(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<ProbeRow, HostError> {
    let mut rows = connection
        .query(
            "SELECT p.operation_id, p.instance_id, p.engine_id, p.docker_root_digest, p.source_revision, p.runtime_image_id, p.image_binding_digest, p.operation_name, p.role, p.projection_digest, p.phase, p.container_id, COALESCE(l.state, 'released'), p.helper_role, p.probe_purpose, p.request_sha256, p.request_context, l.operation_id IS NOT NULL FROM resource_probe_operations AS p LEFT JOIN host_operation_leases AS l ON l.singleton = 1 AND l.operation_id = p.operation_id WHERE p.operation_id = ?1",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let probe = operations::decode_row(&row)?;
    let lease_present = row.get::<i64>(17).map_err(|_| HostError::Journal)? != 0;
    if !lease_present && !matches!(probe.phase, ProbePhase::Removed | ProbePhase::Aborted) {
        return Err(HostError::Journal);
    }
    Ok(probe)
}

pub(in crate::journal::doctor_probe) async fn any_unresolved_probe(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') OR EXISTS (SELECT 1 FROM host_operation_leases WHERE singleton = 1 AND operation_id = resource_probe_operations.operation_id AND state != 'released') LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(in crate::journal::doctor_probe) fn same_seed(seed: &ProbeSeed, row: &ProbeRow) -> bool {
    seed.operation_id == row.operation_id
        && seed.instance_id == row.instance_id
        && seed.engine_id == row.engine_id
        && seed.docker_root_digest == row.docker_root_digest
        && seed.source_revision == row.source_revision
        && seed.runtime_image_id == row.runtime_image_id
        && seed.image_binding_digest == row.image_binding_digest
        && seed.operation_name == row.operation_name
        && row.role == "resource-probe"
        && row.helper_role() == crate::journal::HostHelperRole::ResourceProbe
        && matches!(
            row.work(),
            crate::journal::ProbeWork::ResourceProbe {
                purpose: crate::journal::ResourceProbePurpose::Capacity,
                request_sha256,
                request_context: None,
            } if request_sha256 == &seed.request_sha256
        )
        && seed.projection_digest == row.projection_digest
}

pub(in crate::journal::doctor_probe) fn doctor_seed_valid(seed: &ProbeSeed) -> bool {
    seed.purpose == crate::journal::ResourceProbePurpose::Capacity && operations::seed_valid(seed)
}
