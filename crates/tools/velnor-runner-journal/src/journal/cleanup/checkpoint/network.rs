//! Provisioning-only outer-network reconciliation without permit release.

use crate::error::HostError;

use super::super::{Journal, OuterNetworkRemovalProof};
use super::validation::{container_id, safe_token};

/// Durable journal view of one launch generation's outer network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OuterNetworkCleanupState {
    /// No outer network intent was written.
    NotOwned,
    /// Name was written before create, but the returned network id is unknown.
    NeedsReconciliation {
        /// Deterministic name committed before Docker create.
        name: String,
    },
    /// Exact network id is known; no removal has been requested.
    Bound {
        /// Deterministic name committed before Docker create.
        name: String,
        /// Exact Docker network id bound after create/reconciliation.
        id: String,
    },
    /// Removal intent is durable; a host must finish/reconcile the removal.
    RemovalPending {
        /// Deterministic name committed before Docker create.
        name: String,
        /// Exact id when one was durably bound before the removal fence.
        id: Option<String>,
    },
    /// The host recorded bound-ID or settled deterministic-name absence.
    Absent {
        /// Deterministic name committed before Docker create.
        name: String,
        /// Exact id when one was durably bound before the removal fence.
        id: Option<String>,
    },
}

impl Journal {
    /// Read the durable outer-network state for restart reconciliation.
    ///
    /// `NeedsReconciliation` is not absence: the host must inspect or settle
    /// the deterministic-name create before it may begin removal.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` for an unknown row, malformed identity,
    /// or database failure.
    pub async fn outer_network_cleanup_state(
        &self,
        launch_id: i64,
    ) -> Result<OuterNetworkCleanupState, HostError> {
        if launch_id <= 0 {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        read_state(&conn, launch_id).await
    }

    /// Persist resolution intent for a provisioning-only network.
    ///
    /// With an id, the host has reconciled an existing network and will remove
    /// that exact object. Without an id, the host will finish bounded
    /// deterministic-name reconciliation and prove the create settled absent.
    /// The checkpoint fences later runner/DinD binding and start requests.
    /// This does not resolve a remote acquire or release this launch's permit.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` unless the exact name and id are bound,
    /// the runner pair has not been bound/started, and no full cleanup fence
    /// already owns the generation.
    pub async fn begin_outer_network_removal(
        &self,
        launch_id: i64,
        name: &str,
        network_id: Option<&str>,
    ) -> Result<(), HostError> {
        if self.read_only
            || launch_id <= 0
            || !safe_token(name, 128)
            || network_id.is_some_and(|id| !container_id(id))
        {
            return Err(HostError::Journal);
        }
        self.with_immediate(async |conn| {
            let state = read_state(conn, launch_id).await?;
            match state {
                OuterNetworkCleanupState::Bound { name: found, id }
                    if found == name && Some(id.as_str()) == network_id => {}
                OuterNetworkCleanupState::NeedsReconciliation { name: found }
                    if found == name && network_id.is_none() => {}
                OuterNetworkCleanupState::RemovalPending { name: found, id }
                | OuterNetworkCleanupState::Absent { name: found, id }
                    if found == name && id.as_deref() == network_id => return Ok(()),
                _ => return Err(HostError::Journal),
            }
            ensure_provisioning_only(conn, launch_id, name, network_id).await?;
            conn.execute(
                "INSERT OR IGNORE INTO worker_cleanup_steps (launch_id, step_key, completed) VALUES (?1, 'outer-network-removal', 0)",
                [launch_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Record exact owned-network absence after a durable removal intent.
    ///
    /// This checkpoints only that network. It does not mark worker cleanup
    /// proven, settle Acquire/JIT effects, or free host capacity.
    ///
    /// # Errors
    ///
    /// An id-bearing proof must confirm exact Velnor labels and absence. A
    /// name-only proof must establish that the create effect is settled and
    /// bounded exact-name reconciliation found no network. Neither case
    /// resolves remote job effects or releases capacity.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` unless the proof matches the persisted
    /// identity and a resolution intent is already durable.
    pub async fn record_outer_network_absent<P: OuterNetworkRemovalProof>(
        &self,
        proof: &P,
    ) -> Result<(), HostError> {
        if self.read_only
            || proof.launch_id() <= 0
            || !safe_token(proof.network_name(), 128)
            || !proof.network_absent()
            || match proof.network_id() {
                Some(id) => !container_id(id) || !proof.exact_owned_labels_verified(),
                None => !proof.create_effect_resolved(),
            }
        {
            return Err(HostError::Journal);
        }
        self.with_immediate(async |conn| {
            ensure_provisioning_only(
                conn,
                proof.launch_id(),
                proof.network_name(),
                proof.network_id(),
            )
            .await?;
            let mut rows = conn
                .query(
                    "SELECT completed FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = 'outer-network-removal'",
                    [proof.launch_id()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            let completed = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
            drop(rows);
            if completed == 1 {
                return Ok(());
            }
            if completed != 0 {
                return Err(HostError::Journal);
            }
            let changed = conn
                .execute(
                    "UPDATE worker_cleanup_steps SET completed = 1 WHERE launch_id = ?1 AND step_key = 'outer-network-removal' AND completed = 0",
                    [proof.launch_id()],
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

async fn read_state(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<OuterNetworkCleanupState, HostError> {
    let mut rows = conn
        .query(
            "SELECT outer_network_name, outer_network_id FROM intents WHERE id = ?1 AND kind = 'launch'",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let name = row
        .get::<Option<String>>(0)
        .map_err(|_| HostError::Journal)?;
    let id = row
        .get::<Option<String>>(1)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    match (name, id) {
        (None, None) => Ok(OuterNetworkCleanupState::NotOwned),
        (Some(name), id) if safe_token(&name, 128) && id.as_deref().is_none_or(container_id) => {
            let mut steps = conn
                .query(
                    "SELECT completed FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = 'outer-network-removal'",
                    [launch_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(step) = steps.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(match id {
                    Some(id) => OuterNetworkCleanupState::Bound { name, id },
                    None => OuterNetworkCleanupState::NeedsReconciliation { name },
                });
            };
            match step.get::<i64>(0).map_err(|_| HostError::Journal)? {
                0 => Ok(OuterNetworkCleanupState::RemovalPending { name, id }),
                1 => Ok(OuterNetworkCleanupState::Absent { name, id }),
                _ => Err(HostError::Journal),
            }
        }
        _ => Err(HostError::Journal),
    }
}

async fn ensure_provisioning_only(
    conn: &turso::Connection,
    launch_id: i64,
    name: &str,
    network_id: Option<&str>,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT docker_id, dind_id, runner_start_state, cleanup_proven, outer_network_name, outer_network_id, EXISTS (SELECT 1 FROM worker_cleanup WHERE launch_id = ?1) FROM intents WHERE id = ?1 AND kind = 'launch'",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let valid = row
        .get::<Option<String>>(0)
        .map_err(|_| HostError::Journal)?
        .is_none()
        && row
            .get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<String>>(2)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            == Some("not_requested")
        && row.get::<i64>(3).map_err(|_| HostError::Journal)? == 0
        && row
            .get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            == Some(name)
        && row
            .get::<Option<String>>(5)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            == network_id
        && row.get::<i64>(6).map_err(|_| HostError::Journal)? == 0;
    if valid {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
