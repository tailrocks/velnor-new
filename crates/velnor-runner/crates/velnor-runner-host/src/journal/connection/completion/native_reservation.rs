//! This module reads the native reservation identity for one completed launch.

use crate::error::HostError;
use crate::journal::CompletedLaunch;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;

/// This record keeps the immutable identity and cleanup progress for one native launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeCompletionReservation {
    reservation_id: i64,
    intent_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    remote_cleanup_proven: bool,
    storage_cleanup_proven: bool,
    network_cleanup_proven: bool,
    local_cleanup_proven: bool,
}

impl NativeCompletionReservation {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) const fn remote_cleanup_proven(&self) -> bool {
        self.remote_cleanup_proven
    }

    pub(crate) const fn storage_cleanup_proven(&self) -> bool {
        self.storage_cleanup_proven
    }

    pub(crate) const fn network_cleanup_proven(&self) -> bool {
        self.network_cleanup_proven
    }

    pub(crate) const fn local_cleanup_proven(&self) -> bool {
        self.local_cleanup_proven
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
                    "SELECT r.id, r.intent_id, r.engine_id, r.docker_root_digest, r.state, r.remote_cleanup_proven, r.local_cleanup_proven, i.kind, i.state, i.cleanup_proven, i.runner_completed, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.github_runner_id, i.docker_id, i.dind_id, i.docker_engine_id, c.scale_set_id, c.runner_request_id, c.runner_id, c.runner_name, j.state, j.runner_id, j.runner_name, j.completed_request_id, s.bootstrap_start_state, s.resources_bound, s.jit_state, s.runner_id, s.runner_name, s.runner_scale_set_id, s.runner_container_id, s.dind_container_id, s.instance_binding_digest, s.storage_cleanup_proven, p.operation_id, p.engine_id, p.docker_root_digest, n.operation_id, n.engine_id, n.docker_root_digest, n.create_state, n.remove_state, n.network_name, n.network_id FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN connection_completion_cleanup AS c ON c.intent_id = i.id AND c.connection_id = i.connection_id JOIN connection_runner_jits AS j ON j.intent_id = i.id AND j.connection_id = i.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id AND p.operation_id = r.intent_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id LEFT JOIN capacity_reservation_networks AS n ON n.reservation_id = r.id AND n.connection_id = r.connection_id WHERE r.connection_id = ?1 AND r.intent_id = ?2 AND r.state IN ('reserved', 'uncertain') AND i.kind = 'launch' AND i.cleanup_proven = 0 AND i.runner_completed = 1 AND i.worker_volume IS NULL AND s.bootstrap_start_state = 'started' AND s.resources_bound = 1 AND s.jit_state IN ('bound', 'cleaned') ORDER BY r.id LIMIT 2",
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

            let reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            let intent_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
            let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
            let docker_root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
            let reservation_state: String = row.get(4).map_err(|_| HostError::Journal)?;
            let remote_cleanup_proven: bool = row.get(5).map_err(|_| HostError::Journal)?;
            let local_cleanup_proven: bool = row.get(6).map_err(|_| HostError::Journal)?;
            let kind: String = row.get(7).map_err(|_| HostError::Journal)?;
            let intent_state: String = row.get(8).map_err(|_| HostError::Journal)?;
            let intent_cleanup_proven: bool = row.get(9).map_err(|_| HostError::Journal)?;
            let runner_completed: bool = row.get(10).map_err(|_| HostError::Journal)?;
            let worker_volume: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
            let intent_set_id: Option<i64> = row.get(12).map_err(|_| HostError::Journal)?;
            let intent_request_id: Option<i64> = row.get(13).map_err(|_| HostError::Journal)?;
            let intent_runner_name: Option<String> = row.get(14).map_err(|_| HostError::Journal)?;
            let github_runner_id: Option<String> = row.get(15).map_err(|_| HostError::Journal)?;
            let runner_container_id: Option<String> = row.get(16).map_err(|_| HostError::Journal)?;
            let dind_container_id: Option<String> = row.get(17).map_err(|_| HostError::Journal)?;
            let _legacy_intent_engine_id: Option<String> =
                row.get(18).map_err(|_| HostError::Journal)?;
            let completion_set_id: i64 = row.get(19).map_err(|_| HostError::Journal)?;
            let completion_request_id: i64 = row.get(20).map_err(|_| HostError::Journal)?;
            let completion_runner_id: i64 = row.get(21).map_err(|_| HostError::Journal)?;
            let completion_runner_name: String = row.get(22).map_err(|_| HostError::Journal)?;
            let jit_state: String = row.get(23).map_err(|_| HostError::Journal)?;
            let jit_runner_id: Option<i64> = row.get(24).map_err(|_| HostError::Journal)?;
            let jit_runner_name: Option<String> = row.get(25).map_err(|_| HostError::Journal)?;
            let completed_request_id: Option<i64> = row.get(26).map_err(|_| HostError::Journal)?;
            let bootstrap_state: String = row.get(27).map_err(|_| HostError::Journal)?;
            let resources_bound: bool = row.get(28).map_err(|_| HostError::Journal)?;
            let provisioning_jit_state: String = row.get(29).map_err(|_| HostError::Journal)?;
            let provisioning_runner_id: Option<i64> = row.get(30).map_err(|_| HostError::Journal)?;
            let provisioning_runner_name: Option<String> = row.get(31).map_err(|_| HostError::Journal)?;
            let provisioning_set_id: Option<i64> = row.get(32).map_err(|_| HostError::Journal)?;
            let state_runner_id: Option<String> = row.get(33).map_err(|_| HostError::Journal)?;
            let state_dind_id: Option<String> = row.get(34).map_err(|_| HostError::Journal)?;
            let instance_binding_digest: Option<String> = row.get(35).map_err(|_| HostError::Journal)?;
            let storage_cleanup_proven: bool = row.get(36).map_err(|_| HostError::Journal)?;
            let operation_id: i64 = row.get(37).map_err(|_| HostError::Journal)?;
            let provisioning_engine_id: String = row.get(38).map_err(|_| HostError::Journal)?;
            let provisioning_root_digest: String = row.get(39).map_err(|_| HostError::Journal)?;
            let network_operation_id: Option<i64> = row.get(40).map_err(|_| HostError::Journal)?;
            let network_engine_id: Option<String> = row.get(41).map_err(|_| HostError::Journal)?;
            let network_root_digest: Option<String> = row.get(42).map_err(|_| HostError::Journal)?;
            let network_create_state: Option<String> = row.get(43).map_err(|_| HostError::Journal)?;
            let network_remove_state: Option<String> = row.get(44).map_err(|_| HostError::Journal)?;
            let network_name: Option<String> = row.get(45).map_err(|_| HostError::Journal)?;
            let network_id: Option<String> = row.get(46).map_err(|_| HostError::Journal)?;

            let expected_runner = launch.intent.docker_id.as_deref();
            let expected_dind = launch.intent.dind_id.as_deref();
            let expected_name = launch.intent.runner_name.as_deref();
            let valid = intent_id == launch.intent.id
                && operation_id == intent_id
                && reservation_id > 0
                && matches!(reservation_state.as_str(), "reserved" | "uncertain")
                && kind == "launch"
                && intent_state == launch.intent.state.as_str()
                && !intent_cleanup_proven
                && runner_completed
                && worker_volume.is_none()
                && intent_set_id == Some(launch.identity.scale_set_id)
                && intent_request_id == Some(launch.identity.runner_request_id)
                && intent_runner_name.as_deref() == Some(launch.identity.runner_name.as_str())
                && github_runner_id.as_deref()
                    == Some(launch.identity.runner_id.to_string().as_str())
                && expected_name == Some(launch.identity.runner_name.as_str())
                && expected_runner == runner_container_id.as_deref()
                && expected_dind == dind_container_id.as_deref()
                && completion_set_id == launch.identity.scale_set_id
                && completion_request_id == launch.identity.runner_request_id
                && completion_runner_id == launch.identity.runner_id
                && completion_runner_name == launch.identity.runner_name
                && matches!(jit_state.as_str(), "completed" | "cleaned")
                && jit_runner_id == Some(launch.identity.runner_id)
                && jit_runner_name.as_deref() == Some(launch.identity.runner_name.as_str())
                && completed_request_id == Some(launch.identity.runner_request_id)
                && bootstrap_state == "started"
                && resources_bound
                && matches!(provisioning_jit_state.as_str(), "bound" | "cleaned")
                && provisioning_runner_id == Some(launch.identity.runner_id)
                && provisioning_runner_name.as_deref() == Some(launch.identity.runner_name.as_str())
                && provisioning_set_id == Some(launch.identity.scale_set_id)
                && state_runner_id.as_deref() == expected_runner
                && state_dind_id.as_deref() == expected_dind
                && engine_id == provisioning_engine_id
                && docker_root_digest == provisioning_root_digest
                && network_operation_id == Some(intent_id)
                && network_engine_id.as_deref() == Some(engine_id.as_str())
                && network_root_digest.as_deref() == Some(docker_root_digest.as_str())
                && network_create_state.as_deref() == Some("created")
                && matches!(network_remove_state.as_deref(), Some("not_requested" | "requested" | "uncertain" | "removed"))
                && network_name.as_deref().is_some_and(|name| !name.is_empty())
                && network_id.as_deref().is_some_and(|id| !id.is_empty());
            if !valid {
                return Err(HostError::Identity);
            }
            let instance_binding_digest = instance_binding_digest.ok_or(HostError::Identity)?;
            if instance_binding_digest.len() != 64
                || !instance_binding_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                || docker_root_digest.len() != 64
                || !docker_root_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                || engine_id.is_empty()
                || runner_container_id.as_deref().is_none_or(str::is_empty)
                || dind_container_id.as_deref().is_none_or(str::is_empty)
            {
                return Err(HostError::Identity);
            }
            Ok(Some(NativeCompletionReservation {
                reservation_id,
                intent_id,
                engine_id,
                docker_root_digest,
                runner_container_id: runner_container_id.ok_or(HostError::Identity)?,
                dind_container_id: dind_container_id.ok_or(HostError::Identity)?,
                instance_binding_digest,
                remote_cleanup_proven,
                storage_cleanup_proven,
                network_cleanup_proven: network_remove_state.as_deref() == Some("removed"),
                local_cleanup_proven,
            }))
        })
        .await
    }
}

#[cfg(test)]
#[path = "native_reservation/tests.rs"]
mod tests;
