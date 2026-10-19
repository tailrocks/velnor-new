//! Journal records for Doctor probe requests and their child probes.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::resource_probe::{
    ProbeLeaseState, ProbePhase, ProbeRow, ProbeSeed, lease, operations,
};

use super::{
    DoctorProbeOutcome, DoctorProbePrepareResult, DoctorProbeRequestRecord, DoctorProbeStartResult,
    DoctorProbeState, DoctorProbeTicket,
};

impl Journal {
    /// Record an idempotent Doctor probe request before provider preparation or Docker work.
    pub(crate) async fn start_doctor_probe_request(
        &self,
        nonce: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<DoctorProbeStartResult, HostError> {
        if !lower_hex(nonce, 32)
            || engine_id.is_empty()
            || engine_id.len() > 256
            || engine_id.chars().any(char::is_control)
            || !lower_hex(docker_root_digest, 64)
        {
            return Err(HostError::Journal);
        }
        let nonce = nonce.to_owned();
        let engine_id = engine_id.to_owned();
        let docker_root_digest = docker_root_digest.to_owned();
        in_transaction(self, async |connection| {
            let instance_id = crate::journal_schema::instance_id(connection).await?;
            if crate::journal_schema::engine_id(connection).await? != engine_id {
                return Err(HostError::Journal);
            }
            if let Some(ticket) = ticket_by_nonce(connection, &nonce).await? {
                if ticket.engine_id != engine_id
                    || ticket.docker_root_digest != docker_root_digest
                {
                    return Err(HostError::Journal);
                }
                return Ok(DoctorProbeStartResult::Replayed(DoctorProbeTicket {
                    request_id: ticket.request_id,
                    nonce,
                }));
            }
            if let Some(active_request_id) = active_request_id(connection).await? {
                return Ok(DoctorProbeStartResult::Busy { active_request_id });
            }
            connection
                .execute(
                    "INSERT INTO doctor_probe_requests (nonce, instance_id, engine_id, docker_root_digest, state) VALUES (?1, ?2, ?3, ?4, 'queued')",
                    (nonce.as_str(), instance_id.as_str(), engine_id.as_str(), docker_root_digest.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(DoctorProbeStartResult::Created(DoctorProbeTicket {
                request_id: connection.last_insert_rowid(),
                nonce,
            }))
        })
        .await
    }

    /// Read one ticket by its caller nonce without storing probe output.
    pub(crate) async fn doctor_probe_request_by_nonce(
        &self,
        nonce: &str,
    ) -> Result<Option<DoctorProbeRequestRecord>, HostError> {
        if !lower_hex(nonce, 32) {
            return Err(HostError::Journal);
        }
        read_transaction(self, async |connection| {
            match ticket_by_nonce(connection, nonce).await? {
                Some(ticket) => Ok(Some(record(connection, ticket).await?)),
                None => Ok(None),
            }
        })
        .await
    }

    /// Connect one queued Doctor probe request to its prepared probe and host lease in one database transaction.
    pub(crate) async fn prepare_probe_for_doctor_request(
        &self,
        ticket: &DoctorProbeTicket,
        seed: ProbeSeed,
    ) -> Result<DoctorProbePrepareResult, HostError> {
        let request_id = ticket.request_id();
        let expected_nonce = ticket.nonce().to_owned();
        if request_id <= 0 || !lower_hex(ticket.nonce(), 32) || !doctor_seed_valid(&seed) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let ticket = ticket_by_id(connection, request_id).await?;
            if ticket.nonce != expected_nonce {
                return Err(HostError::Journal);
            }
            if ticket.state != DoctorProbeState::Queued {
                if let Some(operation_id) = ticket.child_operation_id.as_deref() {
                    let child = probe_by_id(connection, operation_id).await?;
                    if ticket.state == DoctorProbeState::Running
                        && child.phase == ProbePhase::Prepared
                        && child.lease_state == ProbeLeaseState::Reserved
                        && same_seed(&seed, &child)
                    {
                        return Ok(DoctorProbePrepareResult::AlreadyLinked(child));
                    }
                }
                return Err(HostError::Journal);
            }
            if ticket.child_operation_id.is_some()
                || ticket.instance_id != seed.instance_id
                || ticket.engine_id != seed.engine_id
                || ticket.docker_root_digest != seed.docker_root_digest
            {
                return Err(HostError::Journal);
            }
            operations::validate_binding(connection, &seed).await?;
            if any_unresolved_probe(connection).await? {
                return Ok(DoctorProbePrepareResult::WaitingForProbe);
            }
            lease::prepare(connection, &seed).await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared', 'resource-probe', 'capacity', ?10, NULL)",
                    (seed.operation_id.as_str(), seed.instance_id.as_str(), seed.engine_id.as_str(), seed.docker_root_digest.as_str(), seed.source_revision.as_str(), seed.runtime_image_id.as_str(), seed.image_binding_digest.as_str(), seed.operation_name.as_str(), seed.projection_digest.as_str(), seed.request_sha256.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET child_operation_id = ?1, state = 'running' WHERE request_id = ?2 AND state = 'queued' AND child_operation_id IS NULL",
                    (seed.operation_id.as_str(), request_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let child = probe_by_id(connection, &seed.operation_id).await?;
            Ok(DoctorProbePrepareResult::Prepared(child))
        })
        .await
    }

    /// Start recovery only for the child probe that the journal connects to this request.
    pub(crate) async fn begin_doctor_probe_recovery(
        &self,
        ticket: &DoctorProbeTicket,
    ) -> Result<ProbeRow, HostError> {
        let request_id = ticket.request_id();
        let expected_nonce = ticket.nonce().to_owned();
        if request_id <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let ticket = ticket_by_id(connection, request_id).await?;
            if ticket.nonce != expected_nonce {
                return Err(HostError::Journal);
            }
            let operation_id = ticket.child_operation_id.ok_or(HostError::Journal)?;
            if !matches!(
                ticket.state,
                DoctorProbeState::Running
                    | DoctorProbeState::Recovering
                    | DoctorProbeState::Uncertain
            ) {
                return Err(HostError::Journal);
            }
            let mut child = probe_by_id(connection, &operation_id).await?;
            if matches!(child.phase, ProbePhase::Removed | ProbePhase::Aborted)
                || child.lease_state == ProbeLeaseState::Released
            {
                return Err(HostError::Journal);
            }
            match child.lease_state {
                ProbeLeaseState::Reserved => {
                    if child.phase != ProbePhase::Prepared {
                        return Err(HostError::Journal);
                    }
                }
                ProbeLeaseState::Active | ProbeLeaseState::Uncertain => {
                    lease::mark_recovering(connection, &operation_id).await?;
                    child.lease_state = ProbeLeaseState::Recovering;
                }
                ProbeLeaseState::Recovering => {}
                ProbeLeaseState::Released => return Err(HostError::Journal),
            }
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET state = 'recovering' WHERE request_id = ?1 AND state IN ('running', 'recovering', 'uncertain') AND child_operation_id = ?2",
                    (request_id, operation_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 && ticket.state != DoctorProbeState::Recovering {
                return Err(HostError::Journal);
            }
            Ok(child)
        })
        .await
    }

    /// If the operation result is uncertain, keep the connected probe and its operational lease charged.
    pub(crate) async fn mark_doctor_probe_uncertain(
        &self,
        ticket: &DoctorProbeTicket,
        operation_id: &str,
    ) -> Result<(), HostError> {
        let request_id = ticket.request_id();
        let expected_nonce = ticket.nonce().to_owned();
        if request_id <= 0 || !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        let operation_id = operation_id.to_owned();
        in_transaction(self, async |connection| {
            let ticket = ticket_by_id(connection, request_id).await?;
            if ticket.nonce != expected_nonce {
                return Err(HostError::Journal);
            }
            if ticket.child_operation_id.as_deref() != Some(operation_id.as_str())
                || !matches!(ticket.state, DoctorProbeState::Running | DoctorProbeState::Recovering | DoctorProbeState::Uncertain)
            {
                return Err(HostError::Journal);
            }
            let child = probe_by_id(connection, &operation_id).await?;
            if matches!(child.phase, ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted) {
                return Err(HostError::Journal);
            }
            if child.phase != ProbePhase::Quarantined {
                connection
                    .execute(
                        "UPDATE resource_probe_operations SET phase = 'quarantined' WHERE operation_id = ?1 AND phase = ?2",
                        (operation_id.as_str(), child.phase.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            if child.lease_state != ProbeLeaseState::Uncertain {
                lease::transition(connection, &operation_id, ProbeLeaseState::Uncertain).await?;
            }
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET state = 'uncertain' WHERE request_id = ?1 AND child_operation_id = ?2 AND state IN ('running', 'recovering', 'uncertain')",
                    (request_id, operation_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || ticket.state == DoctorProbeState::Uncertain {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// After the controller releases the child lease, complete the running Doctor probe request.
    pub(crate) async fn finish_doctor_probe_request(
        &self,
        ticket: &DoctorProbeTicket,
        operation_id: &str,
        outcome: DoctorProbeOutcome,
    ) -> Result<(), HostError> {
        let request_id = ticket.request_id();
        let expected_nonce = ticket.nonce().to_owned();
        if request_id <= 0 || !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        let next = match outcome {
            DoctorProbeOutcome::Failed => DoctorProbeState::Failed,
            DoctorProbeOutcome::Completed => DoctorProbeState::Completed,
            DoctorProbeOutcome::Stale => DoctorProbeState::Stale,
        };
        in_transaction(self, async |connection| {
            let ticket = ticket_by_id(connection, request_id).await?;
            if ticket.nonce != expected_nonce {
                return Err(HostError::Journal);
            }
            if ticket.child_operation_id.as_deref() != Some(operation_id)
                || !matches!(ticket.state, DoctorProbeState::Running | DoctorProbeState::Recovering)
            {
                return Err(HostError::Journal);
            }
            let child = probe_by_id(connection, operation_id).await?;
            if child.lease_state != ProbeLeaseState::Released
                || !matches!(child.phase, ProbePhase::Removed | ProbePhase::Aborted)
                || (next == DoctorProbeState::Completed && child.phase != ProbePhase::Removed)
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET state = ?1 WHERE request_id = ?2 AND child_operation_id = ?3 AND state IN ('running', 'recovering')",
                    (next.as_str(), request_id, operation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Fail a ticket only while it is still queued and has no external child.
    pub(crate) async fn fail_queued_doctor_probe_request(
        &self,
        ticket: &DoctorProbeTicket,
    ) -> Result<(), HostError> {
        let request_id = ticket.request_id();
        let expected_nonce = ticket.nonce().to_owned();
        if request_id <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            if ticket_by_id(connection, request_id).await?.nonce != expected_nonce {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE doctor_probe_requests SET state = 'failed' WHERE request_id = ?1 AND state = 'queued' AND child_operation_id IS NULL",
                    [request_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}

pub(super) struct TicketRow {
    request_id: i64,
    nonce: String,
    instance_id: String,
    engine_id: String,
    docker_root_digest: String,
    state: DoctorProbeState,
    child_operation_id: Option<String>,
}

pub(super) async fn ticket_by_id(
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

async fn ticket_by_nonce(
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

fn decode_ticket(row: &turso::Row) -> Result<TicketRow, HostError> {
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

pub(super) async fn active_request_id(
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

pub(super) async fn record(
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

pub(super) async fn probe_by_id(
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

async fn any_unresolved_probe(connection: &turso::Connection) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') OR EXISTS (SELECT 1 FROM host_operation_leases WHERE singleton = 1 AND operation_id = resource_probe_operations.operation_id AND state != 'released') LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

fn same_seed(seed: &ProbeSeed, row: &ProbeRow) -> bool {
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

fn doctor_seed_valid(seed: &ProbeSeed) -> bool {
    seed.purpose == crate::journal::ResourceProbePurpose::Capacity && operations::seed_valid(seed)
}

#[cfg(test)]
mod validation_tests {
    use crate::journal::{ProbeSeed, ResourceProbePurpose};

    use super::doctor_seed_valid;

    #[test]
    fn doctor_probe_accepts_capacity_work_only() {
        let mut seed = ProbeSeed {
            operation_id: "a".repeat(32),
            instance_id: "b".repeat(32),
            engine_id: "selected-engine".to_owned(),
            docker_root_digest: "c".repeat(64),
            source_revision: "d".repeat(40),
            runtime_image_id: format!("sha256:{}", "e".repeat(64)),
            image_binding_digest: "f".repeat(64),
            operation_name: format!("velnor-resource-probe-{}", "a".repeat(32)),
            projection_digest: "1".repeat(64),
            purpose: ResourceProbePurpose::OwnedCgroups,
            request_sha256: "2".repeat(64),
        };
        assert!(!doctor_seed_valid(&seed));
        seed.purpose = ResourceProbePurpose::Capacity;
        assert!(doctor_seed_valid(&seed));
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
