use crate::error::HostError;
use crate::journal::CompletedLaunch;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;

use super::{LocalCleanupEvidence, NativeCompletionReservation};

struct ReservationCleanupFlags {
    remote: bool,
    local: bool,
}

struct IntentStateFlags {
    cleanup_proven: bool,
    runner_completed: bool,
}

struct ReservationRow {
    reservation_id: i64,
    intent_id: i64,
    engine_id: String,
    docker_root_digest: String,
    reservation_state: String,
    reservation_cleanup: ReservationCleanupFlags,
    kind: String,
    intent_state: String,
    intent_state_flags: IntentStateFlags,
    worker_volume: Option<String>,
    intent_set_id: Option<i64>,
    intent_request_id: Option<i64>,
    intent_runner_name: Option<String>,
    github_runner_id: Option<String>,
    runner_container_id: Option<String>,
    dind_container_id: Option<String>,
    _legacy_intent_engine_id: Option<String>,
    completion_set_id: i64,
    completion_request_id: i64,
    completion_runner_id: i64,
    completion_runner_name: String,
    jit_state: String,
    jit_runner_id: Option<i64>,
    jit_runner_name: Option<String>,
    completed_request_id: Option<i64>,
    bootstrap_state: String,
    resources_bound: bool,
    provisioning_jit_state: String,
    provisioning_runner_id: Option<i64>,
    provisioning_runner_name: Option<String>,
    provisioning_set_id: Option<i64>,
    state_runner_id: Option<String>,
    state_dind_id: Option<String>,
    instance_binding_digest: Option<String>,
    storage_cleanup_proven: bool,
    operation_id: i64,
    provisioning_engine_id: String,
    provisioning_root_digest: String,
    network_operation_id: Option<i64>,
    network_engine_id: Option<String>,
    network_root_digest: Option<String>,
    network_create_state: Option<String>,
    network_remove_state: Option<String>,
    network_name: Option<String>,
    network_id: Option<String>,
}

impl ReservationRow {
    fn read(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
            intent_id: row.get(1).map_err(|_| HostError::Journal)?,
            engine_id: row.get(2).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
            reservation_state: row.get(4).map_err(|_| HostError::Journal)?,
            reservation_cleanup: ReservationCleanupFlags {
                remote: row.get(5).map_err(|_| HostError::Journal)?,
                local: row.get(6).map_err(|_| HostError::Journal)?,
            },
            kind: row.get(7).map_err(|_| HostError::Journal)?,
            intent_state: row.get(8).map_err(|_| HostError::Journal)?,
            intent_state_flags: IntentStateFlags {
                cleanup_proven: row.get(9).map_err(|_| HostError::Journal)?,
                runner_completed: row.get(10).map_err(|_| HostError::Journal)?,
            },
            worker_volume: row.get(11).map_err(|_| HostError::Journal)?,
            intent_set_id: row.get(12).map_err(|_| HostError::Journal)?,
            intent_request_id: row.get(13).map_err(|_| HostError::Journal)?,
            intent_runner_name: row.get(14).map_err(|_| HostError::Journal)?,
            github_runner_id: row.get(15).map_err(|_| HostError::Journal)?,
            runner_container_id: row.get(16).map_err(|_| HostError::Journal)?,
            dind_container_id: row.get(17).map_err(|_| HostError::Journal)?,
            _legacy_intent_engine_id: row.get(18).map_err(|_| HostError::Journal)?,
            completion_set_id: row.get(19).map_err(|_| HostError::Journal)?,
            completion_request_id: row.get(20).map_err(|_| HostError::Journal)?,
            completion_runner_id: row.get(21).map_err(|_| HostError::Journal)?,
            completion_runner_name: row.get(22).map_err(|_| HostError::Journal)?,
            jit_state: row.get(23).map_err(|_| HostError::Journal)?,
            jit_runner_id: row.get(24).map_err(|_| HostError::Journal)?,
            jit_runner_name: row.get(25).map_err(|_| HostError::Journal)?,
            completed_request_id: row.get(26).map_err(|_| HostError::Journal)?,
            bootstrap_state: row.get(27).map_err(|_| HostError::Journal)?,
            resources_bound: row.get(28).map_err(|_| HostError::Journal)?,
            provisioning_jit_state: row.get(29).map_err(|_| HostError::Journal)?,
            provisioning_runner_id: row.get(30).map_err(|_| HostError::Journal)?,
            provisioning_runner_name: row.get(31).map_err(|_| HostError::Journal)?,
            provisioning_set_id: row.get(32).map_err(|_| HostError::Journal)?,
            state_runner_id: row.get(33).map_err(|_| HostError::Journal)?,
            state_dind_id: row.get(34).map_err(|_| HostError::Journal)?,
            instance_binding_digest: row.get(35).map_err(|_| HostError::Journal)?,
            storage_cleanup_proven: row.get(36).map_err(|_| HostError::Journal)?,
            operation_id: row.get(37).map_err(|_| HostError::Journal)?,
            provisioning_engine_id: row.get(38).map_err(|_| HostError::Journal)?,
            provisioning_root_digest: row.get(39).map_err(|_| HostError::Journal)?,
            network_operation_id: row.get(40).map_err(|_| HostError::Journal)?,
            network_engine_id: row.get(41).map_err(|_| HostError::Journal)?,
            network_root_digest: row.get(42).map_err(|_| HostError::Journal)?,
            network_create_state: row.get(43).map_err(|_| HostError::Journal)?,
            network_remove_state: row.get(44).map_err(|_| HostError::Journal)?,
            network_name: row.get(45).map_err(|_| HostError::Journal)?,
            network_id: row.get(46).map_err(|_| HostError::Journal)?,
        })
    }

    fn validate(&self, launch: &CompletedLaunch) -> Result<(), HostError> {
        let expected_runner = launch.intent.docker_id.as_deref();
        let expected_dind = launch.intent.dind_id.as_deref();
        let expected_name = launch.intent.runner_name.as_deref();
        let valid = self.intent_id == launch.intent.id
            && self.operation_id == self.intent_id
            && self.reservation_id > 0
            && matches!(
                self.reservation_state.as_str(),
                "reserved" | "uncertain" | "recovering"
            )
            && self.kind == "launch"
            && self.intent_state == launch.intent.state.as_str()
            && !self.intent_state_flags.cleanup_proven
            && self.intent_state_flags.runner_completed
            && self.worker_volume.is_none()
            && self.intent_set_id == Some(launch.identity.scale_set_id)
            && self.intent_request_id == Some(launch.identity.runner_request_id)
            && self.intent_runner_name.as_deref() == Some(launch.identity.runner_name.as_str())
            && self.github_runner_id.as_deref()
                == Some(launch.identity.runner_id.to_string().as_str())
            && expected_name == Some(launch.identity.runner_name.as_str())
            && expected_runner == self.runner_container_id.as_deref()
            && expected_dind == self.dind_container_id.as_deref()
            && self.completion_set_id == launch.identity.scale_set_id
            && self.completion_request_id == launch.identity.runner_request_id
            && self.completion_runner_id == launch.identity.runner_id
            && self.completion_runner_name == launch.identity.runner_name
            && matches!(self.jit_state.as_str(), "completed" | "cleaned")
            && self.jit_runner_id == Some(launch.identity.runner_id)
            && self.jit_runner_name.as_deref() == Some(launch.identity.runner_name.as_str())
            && self.completed_request_id == Some(launch.identity.runner_request_id)
            && self.bootstrap_state == "started"
            && self.resources_bound
            && matches!(self.provisioning_jit_state.as_str(), "bound" | "cleaned")
            && self.provisioning_runner_id == Some(launch.identity.runner_id)
            && self.provisioning_runner_name.as_deref()
                == Some(launch.identity.runner_name.as_str())
            && self.provisioning_set_id == Some(launch.identity.scale_set_id)
            && self.state_runner_id.as_deref() == expected_runner
            && self.state_dind_id.as_deref() == expected_dind
            && self.engine_id == self.provisioning_engine_id
            && self.docker_root_digest == self.provisioning_root_digest
            && self.network_operation_id == Some(self.intent_id)
            && self.network_engine_id.as_deref() == Some(self.engine_id.as_str())
            && self.network_root_digest.as_deref() == Some(self.docker_root_digest.as_str())
            && self.network_create_state.as_deref() == Some("created")
            && matches!(
                self.network_remove_state.as_deref(),
                Some("not_requested" | "requested" | "uncertain" | "removed")
            )
            && self
                .network_name
                .as_deref()
                .is_some_and(|name| !name.is_empty())
            && self.network_id.as_deref().is_some_and(|id| !id.is_empty());
        if valid {
            Ok(())
        } else {
            Err(HostError::Identity)
        }
    }

    fn into_reservation(self) -> Result<NativeCompletionReservation, HostError> {
        let instance_binding_digest = self.instance_binding_digest.ok_or(HostError::Identity)?;
        if instance_binding_digest.len() != 64
            || !instance_binding_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.docker_root_digest.len() != 64
            || !self
                .docker_root_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.engine_id.is_empty()
            || self
                .runner_container_id
                .as_deref()
                .is_none_or(str::is_empty)
            || self.dind_container_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(HostError::Identity);
        }
        Ok(NativeCompletionReservation {
            reservation_id: self.reservation_id,
            intent_id: self.intent_id,
            engine_id: self.engine_id,
            docker_root_digest: self.docker_root_digest,
            runner_container_id: self.runner_container_id.ok_or(HostError::Identity)?,
            dind_container_id: self.dind_container_id.ok_or(HostError::Identity)?,
            instance_binding_digest,
            remote_cleanup_proven: self.reservation_cleanup.remote,
            local_cleanup_evidence: LocalCleanupEvidence {
                storage: self.storage_cleanup_proven,
                network: self.network_remove_state.as_deref() == Some("removed"),
                local: self.reservation_cleanup.local,
            },
        })
    }
}

impl ConnectionJournal {
    /// Read the charged reservation for the completed native run.
    pub(crate) async fn native_completion_reservation(
        &self,
        launch: &CompletedLaunch,
    ) -> Result<Option<NativeCompletionReservation>, HostError> {
        if launch.intent.id <= 0
            || launch.intent.worker_volume.is_some()
            || launch.identity.scale_set_id <= 0
            || launch.identity.runner_request_id <= 0
            || launch.identity.runner_id <= 0
        {
            return Err(HostError::Identity);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT r.id, r.intent_id, r.engine_id, r.docker_root_digest, r.state, r.remote_cleanup_proven, r.local_cleanup_proven, i.kind, i.state, i.cleanup_proven, i.runner_completed, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.github_runner_id, i.docker_id, i.dind_id, i.docker_engine_id, c.scale_set_id, c.runner_request_id, c.runner_id, c.runner_name, j.state, j.runner_id, j.runner_name, j.completed_request_id, s.bootstrap_start_state, s.resources_bound, s.jit_state, s.runner_id, s.runner_name, s.runner_scale_set_id, s.runner_container_id, s.dind_container_id, s.instance_binding_digest, s.storage_cleanup_proven, p.operation_id, p.engine_id, p.docker_root_digest, n.operation_id, n.engine_id, n.docker_root_digest, n.create_state, n.remove_state, n.network_name, n.network_id FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN connection_completion_cleanup AS c ON c.intent_id = i.id AND c.connection_id = i.connection_id JOIN connection_runner_jits AS j ON j.intent_id = i.id AND j.connection_id = i.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id AND p.operation_id = r.intent_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id LEFT JOIN capacity_reservation_networks AS n ON n.reservation_id = r.id AND n.connection_id = r.connection_id WHERE r.connection_id = ?1 AND r.intent_id = ?2 AND r.state IN ('reserved', 'uncertain', 'recovering') AND i.kind = 'launch' AND i.cleanup_proven = 0 AND i.runner_completed = 1 AND i.worker_volume IS NULL AND s.bootstrap_start_state = 'started' AND s.resources_bound = 1 AND s.jit_state IN ('bound', 'cleaned') ORDER BY r.id LIMIT 2",
                    (self.connection_id().as_str(), launch.intent.id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Identity);
            }

            let decoded = ReservationRow::read(&row)?;
            decoded.validate(launch)?;
            Ok(Some(decoded.into_reservation()?))

        })
        .await
    }
}
