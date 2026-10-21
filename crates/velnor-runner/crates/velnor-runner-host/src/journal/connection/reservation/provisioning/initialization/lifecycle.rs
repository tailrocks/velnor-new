impl ConnectionJournal {
    /// Record the helper ID returned by Docker before inspection.
    pub(crate) async fn record_storage_initializer_create_response(
        &self,
        claim: &StorageInitializationClaim,
        response: &StorageInitializerCreateResponse,
    ) -> Result<(), HostError> {
        if !proof_matches(
            claim,
            response.reservation_id(),
            response.operation_id(),
            response.generation(),
            response.operation_key(),
            response.engine_id(),
            response.docker_root_digest(),
            response.helper_name(),
        ) || !valid_hex(response.container_id(), 64)
        {
            return Err(HostError::Identity);
        }
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET create_state = 'response_recorded', helper_id = ?2 WHERE reservation_id = ?1 AND create_dispatch_state = 'response_received' AND create_state = 'requested' AND helper_id IS NULL", response.container_id(), "helper-id").await
    }

    /// Record the helper ID after the adapter compares its configuration and identity.
    pub(crate) async fn record_storage_initializer_created(
        &self,
        claim: &StorageInitializationClaim,
        proof: &VerifiedStorageInitializerCreated,
    ) -> Result<(), HostError> {
        if !proof_matches(
            claim,
            proof.reservation_id(),
            proof.operation_id(),
            proof.generation(),
            proof.operation_key(),
            proof.engine_id(),
            proof.docker_root_digest(),
            proof.helper_name(),
        ) || !valid_hex(proof.container_id(), 64)
        {
            return Err(HostError::Identity);
        }
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET create_state = 'created' WHERE reservation_id = ?1 AND create_state = 'response_recorded' AND helper_id = ?2", proof.container_id(), "created").await
    }

    /// Record `uncertain` for helper creation when the Docker result is not known.
    #[cfg(test)]
    pub(crate) async fn mark_storage_initializer_create_uncertain(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = CASE WHEN create_dispatch_state = 'dispatched' THEN 'outcome_unknown' ELSE create_dispatch_state END, create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND create_dispatch_state IN ('not_dispatched', 'dispatched', 'response_received', 'outcome_unknown') AND create_state IN ('requested', 'response_recorded', 'created')",
                (claim.reservation_id, connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &connection_id, claim.reservation_id, claim.operation_id).await
        }).await?;
        #[cfg(test)]
        commit_gate::pause_after_commit(claim.reservation_id, "create-uncertain").await;
        Ok(())
    }

    /// Get permission for one start operation after Docker creates the helper.
    pub(crate) async fn claim_storage_initializer_start(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        self.claim_transition(reservation_id, "start_state", "requested", "create_state = 'created' AND start_state = 'not_requested' AND result_state = 'not_requested' AND remove_state = 'not_requested'").await
    }

    /// Record that the identified helper started.
    pub(crate) async fn record_storage_initializer_started(
        &self,
        claim: &StorageInitializationClaim,
        proof: &VerifiedStorageInitializerStarted,
    ) -> Result<(), HostError> {
        if !proof_matches(
            claim,
            proof.reservation_id(),
            proof.operation_id(),
            proof.generation(),
            proof.operation_key(),
            proof.engine_id(),
            proof.docker_root_digest(),
            proof.helper_name(),
        ) {
            return Err(HostError::Identity);
        }
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET start_state = 'started' WHERE reservation_id = ?1 AND create_state = 'created' AND start_state = 'requested' AND helper_id = ?2", proof.container_id(), "started").await
    }

    /// Record `uncertain` for helper start when the Docker result is not known.
    pub(crate) async fn mark_storage_initializer_start_uncertain(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        mark_uncertain(self, claim, "UPDATE capacity_reservation_storage_initialization SET start_state = CASE WHEN start_state = 'requested' THEN 'uncertain' ELSE start_state END WHERE reservation_id = ?1 AND start_state IN ('requested', 'started')").await
    }

    /// Record an exact layout-v2 result with its metadata volume identity.
    pub(crate) async fn record_storage_initializer_result_v2(
        &self,
        claim: &StorageInitializationClaim,
        observed: &ObservedStorageInitialization,
        proof: &crate::worker::storage_initialization::VerifiedStorageInitializerResult,
    ) -> Result<(), HostError> {
        if claim.layout_version != 2
            || proof.layout_version() != 2
            || proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.generation() != claim.generation
            || proof.operation_key() != claim.operation_key
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || Some(proof.metadata_volume_name()) != claim.metadata_volume_name()
            || Some(proof.metadata_volume_id()) != claim.metadata_volume_id()
            || observed.operation_id() != claim.operation_key
            || proof.layout_digest() != observed.layout_digest()
            || proof.ready_digest() != observed.ready_digest()
            || !valid_hex(proof.layout_digest(), 64)
            || !valid_hex(proof.ready_digest(), 64)
        {
            return Err(HostError::Identity);
        }
        let (Some(metadata_name), Some(metadata_id)) =
            (claim.metadata_volume_name(), claim.metadata_volume_id())
        else {
            return Err(HostError::Identity);
        };
        let home_device = observed.home().device();
        let work_device = observed.work().device();
        let values = (
            i64::from(home_device.0),
            i64::from(home_device.1),
            i64::try_from(observed.home().inode()).map_err(|_| HostError::Identity)?,
            i64::try_from(observed.home().mount_id()).map_err(|_| HostError::Identity)?,
            i64::from(work_device.0),
            i64::from(work_device.1),
            i64::try_from(observed.work().inode()).map_err(|_| HostError::Identity)?,
            i64::try_from(observed.work().mount_id()).map_err(|_| HostError::Identity)?,
        );
        let connection_id = self.connection_id().as_str().to_owned();
        let changed = crate::journal::connection::storage::in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET result_state = 'confirmed', home_device_major = ?2, home_device_minor = ?3, home_inode = ?4, home_mount_id = ?5, work_device_major = ?6, work_device_minor = ?7, work_inode = ?8, work_mount_id = ?9, metadata_layout_digest = ?10 WHERE reservation_id = ?1 AND connection_id = ?11 AND operation_id = ?12 AND layout_version = 2 AND metadata_volume = ?13 AND metadata_volume_id = ?14 AND operation_key = ?15 AND engine_id = ?16 AND docker_root_digest = ?17 AND start_state = 'started' AND result_state = 'not_requested' AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata' AND create_state = 'created' AND docker_name = ?13 AND docker_id = ?14)",
                turso::params![claim.reservation_id, values.0, values.1, values.2, values.3, values.4, values.5, values.6, values.7, proof.layout_digest(), connection_id.as_str(), claim.operation_id, metadata_name, metadata_id, claim.operation_key(), claim.engine_id(), claim.docker_root_digest()],
            ).await.map_err(|_| HostError::Journal)
        }).await?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        #[cfg(test)]
        commit_gate::pause_after_commit(claim.reservation_id, "result").await;
        Ok(())
    }

    /// Record `uncertain` when the storage initializer result is incomplete.
    pub(crate) async fn mark_storage_initialization_result_uncertain(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        mark_uncertain(self, claim, "UPDATE capacity_reservation_storage_initialization SET result_state = CASE WHEN result_state = 'not_requested' THEN 'uncertain' ELSE result_state END WHERE reservation_id = ?1 AND start_state = 'started' AND result_state IN ('not_requested', 'confirmed', 'uncertain')").await
    }

    /// Get permission to remove the helper after the storage initializer completes.
    pub(crate) async fn claim_storage_initializer_removal(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        self.claim_transition(reservation_id, "remove_state", "requested", "create_state = 'created' AND start_state = 'started' AND result_state = 'confirmed' AND remove_state = 'not_requested' AND EXISTS (SELECT 1 FROM capacity_reservation_storage_handoffs AS handoff WHERE handoff.reservation_id = capacity_reservation_storage_initialization.reservation_id AND handoff.state = 'attached')").await
    }

    /// Record exact helper absence after Docker inspection.
    pub(crate) async fn record_storage_initializer_removed(
        &self,
        claim: &StorageInitializationClaim,
        proof: &VerifiedStorageInitializerRemoved,
    ) -> Result<(), HostError> {
        if !proof_matches(
            claim,
            proof.reservation_id(),
            proof.operation_id(),
            proof.generation(),
            proof.operation_key(),
            proof.engine_id(),
            proof.docker_root_digest(),
            proof.helper_name(),
        ) {
            return Err(HostError::Identity);
        }
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET remove_state = 'removed' WHERE reservation_id = ?1 AND result_state = 'confirmed' AND remove_state = 'requested' AND helper_id = ?2", proof.container_id(), "removed").await
    }

    /// Record `uncertain` for helper removal when the Docker result is not known.
    pub(crate) async fn mark_storage_initializer_removal_uncertain(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        mark_uncertain(self, claim, "UPDATE capacity_reservation_storage_initialization SET remove_state = CASE WHEN remove_state = 'requested' THEN 'uncertain' ELSE remove_state END WHERE reservation_id = ?1 AND remove_state IN ('requested', 'removed', 'uncertain')").await
    }

    async fn claim_transition(
        &self,
        reservation_id: i64,
        column: &str,
        state: &str,
        predicate: &str,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) {
                return Ok(None);
            }
            let query = format!("SELECT operation_id, generation, operation_key, helper_name, engine_id, docker_root_digest, image_runtime_id, image_source_sha, home_volume, work_volume, nano_cpus, memory_bytes, pids_limit, abort_helper_id, abort_claim_generation, helper_id, start_state, result_state, remove_state, layout_version, metadata_volume, metadata_volume_id FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2 AND {predicate}");
            let mut rows = connection.query(&query, (reservation_id, connection_id.as_str())).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None); };
            let claim = StorageInitializationClaim {
                connection_id: connection_id.clone(), reservation_id,
                operation_id: row.get(0).map_err(|_| HostError::Journal)?, generation: row.get(1).map_err(|_| HostError::Journal)?,
                operation_key: row.get(2).map_err(|_| HostError::Journal)?, helper_name: row.get(3).map_err(|_| HostError::Journal)?,
                engine_id: row.get(4).map_err(|_| HostError::Journal)?, docker_root_digest: row.get(5).map_err(|_| HostError::Journal)?,
                image_runtime_id: row.get(6).map_err(|_| HostError::Journal)?, image_source_sha: row.get(7).map_err(|_| HostError::Journal)?,
                home_volume: row.get(8).map_err(|_| HostError::Journal)?, work_volume: row.get(9).map_err(|_| HostError::Journal)?,
                nano_cpus: row.get(10).map_err(|_| HostError::Journal)?, memory_bytes: row.get(11).map_err(|_| HostError::Journal)?, pids_limit: row.get(12).map_err(|_| HostError::Journal)?, abort_helper_id: row.get(13).map_err(|_| HostError::Journal)?, abort_claim_generation: row.get(14).map_err(|_| HostError::Journal)?, helper_id: row.get(15).map_err(|_| HostError::Journal)?,
                layout_version: row.get(19).map_err(|_| HostError::Journal)?, metadata_volume: row.get(20).map_err(|_| HostError::Journal)?, metadata_volume_id: row.get(21).map_err(|_| HostError::Journal)?,
            };
            if claim.operation_id != context.operation_id
                || claim.engine_id != context.engine_id
                || claim.docker_root_digest != context.docker_root_digest
            {
                return Err(HostError::Journal);
            }
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal); }
            let update = match column { "start_state" => "UPDATE capacity_reservation_storage_initialization SET start_state = ?2 WHERE reservation_id = ?1", "remove_state" => "UPDATE capacity_reservation_storage_initialization SET remove_state = ?2 WHERE reservation_id = ?1", _ => return Err(HostError::Journal) };
            let changed = connection.execute(update, (reservation_id, state)).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            Ok(Some(claim))
        }).await
    }
}
