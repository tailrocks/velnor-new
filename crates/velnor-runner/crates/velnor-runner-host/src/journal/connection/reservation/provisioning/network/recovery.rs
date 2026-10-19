use super::{
    ConnectionJournal, HostError, StorageNetworkCreateClaim, StorageNetworkRecoveryIdentity,
    StorageNetworkRemovalClaim, VerifiedStorageNetworkRemoved, in_transaction, parse_labels,
    validate_create_claim, validate_remove_claim,
};

impl ConnectionJournal {
    /// Keep the capacity hold when Docker did not return a verifiable result.
    pub(crate) async fn mark_storage_network_create_uncertain(
        &self,
        claim: &StorageNetworkCreateClaim,
    ) -> Result<(), HostError> {
        validate_create_claim(self, claim)?;
        self.set_network_state(
            claim.reservation_id,
            "create_state",
            "uncertain",
            "requested",
        )
        .await
    }

    /// Load the durable identity for explicit recovery after a restart.
    pub(crate) async fn storage_network_recovery_identity(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageNetworkRecoveryIdentity>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        crate::journal::connection::storage::read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection.query(
                "SELECT operation_id, engine_id, docker_root_digest, network_name, network_id, labels_json, create_state, remove_state FROM capacity_reservation_networks WHERE reservation_id = ?1 AND connection_id = ?2",
                (reservation_id, connection_id.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None); };
            let labels_json: String = row.get(5).map_err(|_| HostError::Journal)?;
            let labels = parse_labels(&labels_json)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal); }
            Ok(Some(StorageNetworkRecoveryIdentity {
                reservation_id, operation_id: row.get(0).map_err(|_| HostError::Journal)?,
                engine_id: row.get(1).map_err(|_| HostError::Journal)?,
                docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
                network_name: row.get(3).map_err(|_| HostError::Journal)?,
                network_id: row.get(4).map_err(|_| HostError::Journal)?, labels,
                create_state: row.get(6).map_err(|_| HostError::Journal)?,
                remove_state: row.get(7).map_err(|_| HostError::Journal)?,
            }))
        }).await
    }

    /// Restore the exact pending create identity after a journal reopen.
    pub(crate) async fn recover_storage_network_create(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageNetworkCreateClaim>, HostError> {
        let Some(identity) = self
            .storage_network_recovery_identity(reservation_id)
            .await?
        else {
            return Ok(None);
        };
        if !matches!(identity.create_state(), "requested" | "uncertain")
            || identity.network_id().is_some()
            || identity.remove_state() != "not_requested"
        {
            return Ok(None);
        }
        Ok(Some(StorageNetworkCreateClaim {
            connection_id: self.connection_id().as_str().to_owned(),
            reservation_id: identity.reservation_id,
            operation_id: identity.operation_id,
            engine_id: identity.engine_id,
            docker_root_digest: identity.docker_root_digest,
            network_name: identity.network_name,
            labels: identity.labels,
        }))
    }

    /// Restore a pending removal identity after a journal reopen.
    pub(crate) async fn recover_storage_network_removal(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageNetworkRemovalClaim>, HostError> {
        let Some(identity) = self
            .storage_network_recovery_identity(reservation_id)
            .await?
        else {
            return Ok(None);
        };
        if !matches!(identity.remove_state(), "requested" | "uncertain") {
            return Ok(None);
        }
        Ok(Some(StorageNetworkRemovalClaim {
            connection_id: self.connection_id().as_str().to_owned(),
            reservation_id: identity.reservation_id,
            operation_id: identity.operation_id,
            engine_id: identity.engine_id,
            docker_root_digest: identity.docker_root_digest,
            network_name: identity.network_name,
            network_id: identity.network_id,
            labels: identity.labels,
        }))
    }

    /// Claim network removal after the worker cleanup proof is durable.
    pub(crate) async fn claim_storage_network_removal(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageNetworkRemovalClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection.query(
                "SELECT n.operation_id, n.engine_id, n.docker_root_digest, n.network_name, n.network_id, n.labels_json, n.create_state FROM capacity_reservation_networks AS n JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = n.reservation_id WHERE n.reservation_id = ?1 AND n.connection_id = ?2 AND n.remove_state = 'not_requested' AND (s.storage_cleanup_proven = 1 OR (s.resources_bound = 0 AND s.bootstrap_start_state = 'not_requested' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects AS o WHERE o.reservation_id = n.reservation_id AND o.create_state != 'unclaimed') AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS b WHERE b.reservation_id = n.reservation_id AND (b.create_state != 'unclaimed' OR b.start_state != 'not_requested')) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS x WHERE x.reservation_id = n.reservation_id)))",
                (reservation_id, connection_id.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None); };
            let operation_id = row.get(0).map_err(|_| HostError::Journal)?;
            let engine_id = row.get(1).map_err(|_| HostError::Journal)?;
            let docker_root_digest = row.get(2).map_err(|_| HostError::Journal)?;
            let network_name = row.get(3).map_err(|_| HostError::Journal)?;
            let network_id = row.get(4).map_err(|_| HostError::Journal)?;
            let labels = parse_labels(&row.get::<String>(5).map_err(|_| HostError::Journal)?)?;
            let create_state: String = row.get(6).map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal); }
            let changed = connection.execute(
                "UPDATE capacity_reservation_networks SET remove_state = 'requested' WHERE reservation_id = ?1 AND connection_id = ?2 AND remove_state = 'not_requested' AND create_state = ?3",
                (reservation_id, connection_id.as_str(), create_state.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            Ok(Some(StorageNetworkRemovalClaim { connection_id: connection_id.clone(), reservation_id, operation_id, engine_id, docker_root_digest, network_name, network_id, labels }))
        }).await
    }

    /// Record exact absence from the selected daemon after the delete request.
    pub(crate) async fn record_storage_network_removed(
        &self,
        claim: &StorageNetworkRemovalClaim,
        proof: &VerifiedStorageNetworkRemoved,
    ) -> Result<(), HostError> {
        validate_remove_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.network_name() != claim.network_name
            || proof.network_id() != claim.network_id.as_deref()
            || proof.labels() != &claim.labels
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_networks SET remove_state = 'removed' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND network_name = ?4 AND network_id IS ?5 AND remove_state = 'requested'",
                (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.network_name.as_str(), claim.network_id.as_deref()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        }).await
    }

    /// Keep the reservation charged when network removal is uncertain.
    pub(crate) async fn mark_storage_network_removal_uncertain(
        &self,
        claim: &StorageNetworkRemovalClaim,
    ) -> Result<(), HostError> {
        validate_remove_claim(self, claim)?;
        self.set_network_state(
            claim.reservation_id,
            "remove_state",
            "uncertain",
            "requested",
        )
        .await
    }

    async fn set_network_state(
        &self,
        reservation_id: i64,
        column: &str,
        next: &str,
        prior: &str,
    ) -> Result<(), HostError> {
        let sql = match column {
            "create_state" => {
                "UPDATE capacity_reservation_networks SET create_state = ?1 WHERE reservation_id = ?2 AND connection_id = ?3 AND create_state = ?4 AND network_id IS NULL"
            }
            "remove_state" => {
                "UPDATE capacity_reservation_networks SET remove_state = ?1 WHERE reservation_id = ?2 AND connection_id = ?3 AND remove_state = ?4"
            }
            _ => return Err(HostError::Identity),
        };
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(sql, (next, reservation_id, connection_id.as_str(), prior))
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

pub(in crate::journal::connection::reservation::provisioning) async fn release_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT remove_state FROM capacity_reservation_networks WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(true);
    };
    let remove_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(remove_state == "removed")
}
