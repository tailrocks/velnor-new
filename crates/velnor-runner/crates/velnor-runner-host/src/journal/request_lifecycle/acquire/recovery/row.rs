//! Read and check row values for recovery of an acquired request.

use crate::error::HostError;
use crate::journal::connection::{AssignmentIdentity, AssignmentKey, ConnectionJournal};

use super::{AcquiredRecoveryCandidate, AcquiredRecoveryLookup};

pub(super) async fn load_candidate_row(
    connection: &turso::Connection,
    lookup: &AcquiredRecoveryLookup<'_>,
) -> Result<Option<turso::Row>, HostError> {
    let mut rows = connection
        .query(
            "SELECT r.id, r.intent_id, r.assignment_key, r.engine_id, r.docker_root_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, i.assignment_key, i.runner_name, i.state, i.cleanup_proven, i.scale_set_id, i.runner_request_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.github_runner_id, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, p.operation_id, p.engine_id, p.docker_root_digest, p.sample_generation_id, s.bootstrap_start_state, s.resources_bound, s.jit_state, s.publication_state, s.delivery_state, s.storage_cleanup_proven, attempt.physical_nonce, p.profile_generation_id, p.profile_digest FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id AND p.operation_id = i.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN physical_attempt_current AS current ON current.reservation_id = r.id AND current.intent_id = i.id JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id WHERE attempt.witness_request_id = ?1 AND r.id = ?2 AND r.connection_id = ?3 AND r.binding_digest = ?4 AND r.state = 'reserved' AND i.scale_set_id = ?5 AND attempt.connection_id = r.connection_id AND attempt.binding_digest = ?4",
            (
                lookup.request_id,
                lookup.reservation_id,
                lookup.connection_id.as_str(),
                lookup.binding_digest.as_str(),
                lookup.scale_set_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(row))
}

pub(super) async fn candidate_from_row(
    scoped: &ConnectionJournal,
    connection: &turso::Connection,
    lookup: &AcquiredRecoveryLookup<'_>,
    row: &turso::Row,
) -> Result<Option<AcquiredRecoveryCandidate>, HostError> {
    let row = AcquiredRecoveryRow::read(scoped, lookup, row)?;
    row.into_candidate(connection, lookup).await
}

struct AcquiredRecoveryRow {
    reservation_id: i64,
    reservation: AcquiredReservationRow,
    intent: AcquiredIntentRow,
    provisioning: AcquiredProvisioningRow,
    physical_assignment: AssignmentIdentity,
}

impl AcquiredRecoveryRow {
    fn read(
        scoped: &ConnectionJournal,
        lookup: &AcquiredRecoveryLookup<'_>,
        row: &turso::Row,
    ) -> Result<Self, HostError> {
        let reservation = AcquiredReservationRow::read(row)?;
        let intent = AcquiredIntentRow::read(row)?;
        let provisioning = AcquiredProvisioningRow::read(row)?;
        let physical_assignment = physical_assignment(scoped, lookup, &provisioning.nonce)?;
        let reservation_id = row.get(0).map_err(|_| HostError::Journal)?;
        Ok(Self {
            reservation_id,
            reservation,
            intent,
            provisioning,
            physical_assignment,
        })
    }

    async fn into_candidate(
        self,
        connection: &turso::Connection,
        lookup: &AcquiredRecoveryLookup<'_>,
    ) -> Result<Option<AcquiredRecoveryCandidate>, HostError> {
        if !self.identity_matches(lookup)
            || !self.intent_is_pristine()
            || !self.provisioning_is_pristine()
            || !super::pristine_rows(connection, lookup.reservation_id).await?
        {
            return Ok(None);
        }
        Ok(Some(AcquiredRecoveryCandidate {
            request: lookup.request.clone(),
            #[cfg(test)]
            physical_assignment: self.physical_assignment,
            reservation_id: lookup.reservation_id,
            intent_id: self.reservation.intent_id,
            runner_name: self.intent.runner_name,
            engine_id: self.reservation.engine_id,
            docker_root_digest: self.reservation.docker_root_digest,
            sample_generation_id: self.reservation.sample_generation_id,
            storage_profile_generation_id: self.provisioning.profile_generation_id,
            storage_profile_digest: self.provisioning.profile_digest,
            policy_generation: lookup.policy_generation,
            policy_digest: lookup.policy_digest.clone(),
            charge: (self.reservation.cpu, self.reservation.memory),
        }))
    }

    fn identity_matches(&self, lookup: &AcquiredRecoveryLookup<'_>) -> bool {
        self.reservation_id == lookup.reservation_id
            && self.reservation.intent_id == self.provisioning.operation_id
            && self.reservation.reservation_key == self.intent.intent_key
            && self.reservation.reservation_key == self.physical_assignment.storage_key()
            && self.intent.intent_set_id == lookup.scale_set_id
            && !self.intent.cleanup_proven
            && self.intent.intent_state == "pending"
    }

    fn intent_is_pristine(&self) -> bool {
        let (acquire_attempted, acquire_resolved, acquired, jit_requested) =
            self.intent.acquire_state;
        self.intent.intent_request_id.is_none()
            && !acquire_attempted
            && !acquire_resolved
            && !acquired
            && !jit_requested
            && self.intent.github_runner_id.is_none()
            && self.intent.docker_id.is_none()
            && self.intent.dind_id.is_none()
            && self.intent.worker_volume.is_none()
            && self.intent.launch_phase.as_deref() == Some("provisioning_held")
    }

    fn provisioning_is_pristine(&self) -> bool {
        self.provisioning.engine_id == self.reservation.engine_id
            && self.provisioning.docker_root_digest == self.reservation.docker_root_digest
            && self.provisioning.sample_generation_id == self.reservation.sample_generation_id
            && self.provisioning.bootstrap_state == "not_requested"
            && !self.provisioning.resources_bound
            && self.provisioning.jit_state == "not_requested"
            && self.provisioning.publication_state == "not_requested"
            && self.provisioning.delivery_state == "not_requested"
            && !self.provisioning.storage_cleanup_proven
            && nonce_valid(&self.provisioning.nonce)
            && self.intent.runner_name == format!("v{}", self.provisioning.nonce)
    }
}

struct AcquiredReservationRow {
    intent_id: i64,
    reservation_key: String,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    cpu: u64,
    memory: u64,
}

impl AcquiredReservationRow {
    fn read(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            intent_id: row.get(1).map_err(|_| HostError::Journal)?,
            reservation_key: row.get(2).map_err(|_| HostError::Journal)?,
            engine_id: row.get(3).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(4).map_err(|_| HostError::Journal)?,
            sample_generation_id: row.get(5).map_err(|_| HostError::Journal)?,
            cpu: read_charge(row.get(6).map_err(|_| HostError::Journal)?)?,
            memory: read_charge(row.get(7).map_err(|_| HostError::Journal)?)?,
        })
    }
}

struct AcquiredIntentRow {
    intent_key: String,
    runner_name: String,
    intent_state: String,
    cleanup_proven: bool,
    intent_set_id: i64,
    intent_request_id: Option<i64>,
    acquire_state: (bool, bool, bool, bool),
    github_runner_id: Option<String>,
    docker_id: Option<String>,
    dind_id: Option<String>,
    worker_volume: Option<String>,
    launch_phase: Option<String>,
}

impl AcquiredIntentRow {
    fn read(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            intent_key: row.get(8).map_err(|_| HostError::Journal)?,
            runner_name: row.get(9).map_err(|_| HostError::Journal)?,
            intent_state: row.get(10).map_err(|_| HostError::Journal)?,
            cleanup_proven: row.get(11).map_err(|_| HostError::Journal)?,
            intent_set_id: row.get(12).map_err(|_| HostError::Journal)?,
            intent_request_id: row.get(13).map_err(|_| HostError::Journal)?,
            acquire_state: (
                row.get(14).map_err(|_| HostError::Journal)?,
                row.get(15).map_err(|_| HostError::Journal)?,
                row.get(16).map_err(|_| HostError::Journal)?,
                row.get(17).map_err(|_| HostError::Journal)?,
            ),
            github_runner_id: row.get(18).map_err(|_| HostError::Journal)?,
            docker_id: row.get(19).map_err(|_| HostError::Journal)?,
            dind_id: row.get(20).map_err(|_| HostError::Journal)?,
            worker_volume: row.get(21).map_err(|_| HostError::Journal)?,
            launch_phase: row.get(22).map_err(|_| HostError::Journal)?,
        })
    }
}

struct AcquiredProvisioningRow {
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    bootstrap_state: String,
    resources_bound: bool,
    jit_state: String,
    publication_state: String,
    delivery_state: String,
    storage_cleanup_proven: bool,
    nonce: String,
    profile_generation_id: String,
    profile_digest: String,
}

impl AcquiredProvisioningRow {
    fn read(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            operation_id: row.get(23).map_err(|_| HostError::Journal)?,
            engine_id: row.get(24).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(25).map_err(|_| HostError::Journal)?,
            sample_generation_id: row.get(26).map_err(|_| HostError::Journal)?,
            bootstrap_state: row.get(27).map_err(|_| HostError::Journal)?,
            resources_bound: row.get(28).map_err(|_| HostError::Journal)?,
            jit_state: row.get(29).map_err(|_| HostError::Journal)?,
            publication_state: row.get(30).map_err(|_| HostError::Journal)?,
            delivery_state: row.get(31).map_err(|_| HostError::Journal)?,
            storage_cleanup_proven: row.get(32).map_err(|_| HostError::Journal)?,
            nonce: row.get(33).map_err(|_| HostError::Journal)?,
            profile_generation_id: row.get(34).map_err(|_| HostError::Journal)?,
            profile_digest: row.get(35).map_err(|_| HostError::Journal)?,
        })
    }
}

fn physical_assignment(
    scoped: &ConnectionJournal,
    lookup: &AcquiredRecoveryLookup<'_>,
    nonce: &str,
) -> Result<AssignmentIdentity, HostError> {
    AssignmentIdentity::new(
        scoped.connection_id(),
        lookup.scale_set_id,
        AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_owned(),
        },
    )
}

fn read_charge(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}

fn nonce_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
