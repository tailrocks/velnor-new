use crate::error::HostError;
use crate::journal::connection::{ConnectionJournal, storage::read_transaction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativePublicationSnapshot {
    pub(crate) reservation_id: i64,
    pub(crate) operation_id: i64,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) resources_bound: bool,
    pub(crate) jit_state: String,
    pub(crate) publication_state: String,
    pub(crate) projection_sha256: Option<String>,
    pub(crate) delivery_state: String,
    pub(crate) challenge_state: Option<String>,
    pub(crate) challenge_digest: Option<String>,
}

impl ConnectionJournal {
    pub(crate) async fn native_publication_snapshot(
        &self,
        reservation_id: i64,
    ) -> Result<Option<NativePublicationSnapshot>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT provisioning.operation_id, reservation.engine_id, reservation.docker_root_digest, state.instance_binding_digest, state.runner_container_id, state.dind_container_id, state.resources_bound, state.jit_state, state.publication_state, state.projection_sha256, state.delivery_state, proof.challenge_state, proof.challenge_digest FROM capacity_reservation_provisioning_state AS state JOIN capacity_reservations AS reservation ON reservation.id = state.reservation_id AND reservation.connection_id = ?2 JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = state.reservation_id LEFT JOIN connection_execution_proofs AS proof ON proof.reservation_id = state.reservation_id AND proof.intent_id = provisioning.operation_id AND proof.connection_id = reservation.connection_id WHERE state.reservation_id = ?1",
                    (reservation_id, connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let snapshot = NativePublicationSnapshot {
                reservation_id,
                operation_id: row.get(0).map_err(|_| HostError::Journal)?,
                engine_id: row.get(1).map_err(|_| HostError::Journal)?,
                docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
                instance_binding_digest: row.get(3).map_err(|_| HostError::Journal)?,
                runner_container_id: row.get(4).map_err(|_| HostError::Journal)?,
                dind_container_id: row.get(5).map_err(|_| HostError::Journal)?,
                resources_bound: row.get::<i64>(6).map_err(|_| HostError::Journal)? == 1,
                jit_state: row.get(7).map_err(|_| HostError::Journal)?,
                publication_state: row.get(8).map_err(|_| HostError::Journal)?,
                projection_sha256: row.get(9).map_err(|_| HostError::Journal)?,
                delivery_state: row.get(10).map_err(|_| HostError::Journal)?,
                challenge_state: row.get(11).map_err(|_| HostError::Journal)?,
                challenge_digest: row.get(12).map_err(|_| HostError::Journal)?,
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(snapshot))
        })
        .await
    }
}
