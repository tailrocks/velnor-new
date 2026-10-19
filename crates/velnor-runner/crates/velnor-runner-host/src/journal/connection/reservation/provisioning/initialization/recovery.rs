impl ConnectionJournal {
    /// Read a candidate before the adapter checks the selected engine.
    pub(crate) async fn storage_initializer_abort_candidate(
        &self,
        reservation_id: i64,
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
            let prebind = prebind_effects_absent(connection, reservation_id).await?;
            if !prebind {
                return Ok(None);
            }
            let now = unix_seconds()?;
            let mut rows = connection.query(
                "SELECT operation_id, generation, operation_key, helper_name, engine_id, docker_root_digest, image_runtime_id, image_source_sha, home_volume, work_volume, nano_cpus, memory_bytes, pids_limit, abort_helper_id, abort_claim_generation, abort_lease_until, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, helper_id, layout_version, metadata_volume, metadata_volume_id FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2",
                (reservation_id, connection_id.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let dispatch = row.get::<String>(16).map_err(|_| HostError::Journal)?;
            let create_state = row.get::<String>(17).map_err(|_| HostError::Journal)?;
            let dispatched_hold = context.reservation_state == "reserved"
                && context.intent_state == "pending"
                && matches!(dispatch.as_str(), "dispatched" | "response_received")
                && matches!(create_state.as_str(), "requested" | "response_recorded" | "created")
                && row.get::<String>(21).map_err(|_| HostError::Journal)? == "not_requested";
            let pre_dispatch_hold = context.reservation_state == "reserved"
                && context.intent_state == "pending"
                && dispatch == "not_dispatched"
                && create_state == "requested"
                && row.get::<String>(21).map_err(|_| HostError::Journal)? == "not_requested";
            let uncertain_hold = context.reservation_state == "uncertain"
                && context.intent_state == "uncertain"
                && matches!(dispatch.as_str(), "not_dispatched" | "response_received" | "outcome_unknown");
            let recoverable = row.get::<i64>(15).map_err(|_| HostError::Journal)? <= now
                && (dispatched_hold || pre_dispatch_hold || uncertain_hold)
                && matches!(dispatch.as_str(), "not_dispatched" | "dispatched" | "response_received" | "outcome_unknown")
                && matches!(row.get::<String>(17).map_err(|_| HostError::Journal)?.as_str(), "requested" | "response_recorded" | "created" | "uncertain")
                && matches!(row.get::<String>(18).map_err(|_| HostError::Journal)?.as_str(), "not_requested" | "requested" | "started" | "uncertain")
                && matches!(row.get::<String>(19).map_err(|_| HostError::Journal)?.as_str(), "not_requested" | "confirmed" | "uncertain")
                && matches!(row.get::<String>(20).map_err(|_| HostError::Journal)?.as_str(), "not_requested" | "requested" | "removed" | "uncertain")
                && matches!(row.get::<String>(21).map_err(|_| HostError::Journal)?.as_str(), "not_requested" | "requested" | "remove_requested" | "uncertain");
            if !recoverable { return Ok(None); }
            let claim = StorageInitializationClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: row.get(0).map_err(|_| HostError::Journal)?,
                generation: row.get(1).map_err(|_| HostError::Journal)?,
                operation_key: row.get(2).map_err(|_| HostError::Journal)?,
                helper_name: row.get(3).map_err(|_| HostError::Journal)?,
                engine_id: row.get(4).map_err(|_| HostError::Journal)?,
                docker_root_digest: row.get(5).map_err(|_| HostError::Journal)?,
                image_runtime_id: row.get(6).map_err(|_| HostError::Journal)?,
                image_source_sha: row.get(7).map_err(|_| HostError::Journal)?,
                home_volume: row.get(8).map_err(|_| HostError::Journal)?,
                work_volume: row.get(9).map_err(|_| HostError::Journal)?,
                nano_cpus: row.get(10).map_err(|_| HostError::Journal)?,
                memory_bytes: row.get(11).map_err(|_| HostError::Journal)?,
                pids_limit: row.get(12).map_err(|_| HostError::Journal)?,
                helper_id: row.get(22).map_err(|_| HostError::Journal)?,
                layout_version: row.get(23).map_err(|_| HostError::Journal)?,
                metadata_volume: row.get(24).map_err(|_| HostError::Journal)?,
                metadata_volume_id: row.get(25).map_err(|_| HostError::Journal)?,
                start_state: row.get(18).map_err(|_| HostError::Journal)?,
                result_state: row.get(19).map_err(|_| HostError::Journal)?,
                remove_state: row.get(20).map_err(|_| HostError::Journal)?,
                abort_helper_id: row.get(13).map_err(|_| HostError::Journal)?,
                abort_claim_generation: row.get(14).map_err(|_| HostError::Journal)?,
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(claim))
        }).await
    }

    /// Record recovery permission after the adapter checks the selected engine.
    pub(crate) async fn claim_storage_initializer_abort(
        &self,
        candidate: &StorageInitializationClaim,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        if self.connection_id().as_str() != candidate.connection_id || candidate.reservation_id <= 0
        {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, candidate.reservation_id).await? else {
                return Ok(None);
            };
            if !prebind_effects_absent(connection, candidate.reservation_id).await? {
                return Ok(None);
            }
            let now = unix_seconds()?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = CASE WHEN create_dispatch_state = 'dispatched' THEN 'outcome_unknown' ELSE create_dispatch_state END, create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND abort_claim_generation = ?9 AND abort_lease_until <= ?10 AND create_dispatch_state IN ('not_dispatched', 'dispatched', 'response_received') AND create_state IN ('requested', 'response_recorded', 'created') AND abort_state = 'not_requested' AND ?11 = 'reserved' AND ?12 = 'pending'",
                (candidate.reservation_id, connection_id.as_str(), candidate.operation_id, candidate.generation, candidate.engine_id.as_str(), candidate.docker_root_digest.as_str(), candidate.operation_key.as_str(), candidate.helper_name.as_str(), candidate.abort_claim_generation, now, context.reservation_state.as_str(), context.intent_state.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed == 1 {
                mark_hold_uncertain(connection, &connection_id, candidate.reservation_id, candidate.operation_id).await?;
            } else if context.reservation_state != "uncertain"
                || context.intent_state != "uncertain"
            {
                return Ok(None);
            }
            let mut claim = (*candidate).clone();
            let next_generation = claim.abort_claim_generation.checked_add(1).ok_or(HostError::Journal)?;
            let lease_until = now.checked_add(45).ok_or(HostError::Journal)?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET abort_claim_generation = ?3 WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?4 AND generation = ?5 AND engine_id = ?6 AND docker_root_digest = ?7 AND operation_key = ?8 AND helper_name = ?9 AND abort_claim_generation = ?10 AND abort_lease_until <= ?11 AND create_dispatch_state IN ('not_dispatched', 'response_received', 'outcome_unknown') AND abort_state IN ('not_requested', 'requested', 'remove_requested', 'uncertain')",
                (claim.reservation_id, connection_id.as_str(), next_generation, claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str(), claim.abort_claim_generation, now),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET abort_lease_until = ?3 WHERE reservation_id = ?1 AND connection_id = ?2 AND abort_claim_generation = ?4 AND abort_lease_until <= ?5",
                (claim.reservation_id, connection_id.as_str(), lease_until, next_generation, now),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET abort_state = CASE WHEN abort_state = 'remove_requested' THEN 'remove_requested' ELSE 'requested' END WHERE reservation_id = ?1 AND connection_id = ?2 AND abort_claim_generation = ?3 AND abort_lease_until = ?4 AND create_dispatch_state IN ('not_dispatched', 'response_received', 'outcome_unknown') AND abort_state IN ('not_requested', 'requested', 'remove_requested', 'uncertain')",
                (claim.reservation_id, connection_id.as_str(), next_generation, lease_until),
            ).await.map_err(|_| HostError::Journal)?;
            claim.abort_claim_generation = next_generation;
            if changed != 1 { return Err(HostError::Journal); }
            Ok(Some(claim))
        }).await
    }

    /// Record the exact helper ID before the adapter removes that helper.
    pub(crate) async fn record_storage_initializer_abort_removal_requested(
        &self,
        claim: &StorageInitializationClaim,
        proof: &crate::worker::storage_initialization::VerifiedStorageInitializerAbortRemoval,
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
        ) || proof.abort_claim_generation() != claim.abort_claim_generation
            || !valid_hex(proof.container_id(), 64)
            || claim
                .abort_helper_id()
                .is_some_and(|helper_id| helper_id != proof.container_id())
            || claim
                .helper_id()
                .is_some_and(|helper_id| helper_id != proof.container_id())
        {
            return Err(HostError::Identity);
        }
        update_abort_claim(
            self, claim,
            "UPDATE capacity_reservation_storage_initialization SET abort_helper_id = ?2, abort_state = 'remove_requested' WHERE reservation_id = ?1 AND abort_state IN ('requested', 'remove_requested') AND (abort_helper_id IS NULL OR abort_helper_id = ?2)",
            proof.container_id(),
            "abort-removal",
        ).await
    }

    /// Record exact absence after the adapter compares the selected engine and the volume identities.
    pub(crate) async fn record_storage_initializer_absent(
        &self,
        claim: &StorageInitializationClaim,
        proof: &VerifiedStorageInitializerAbsent,
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
        ) || proof.abort_claim_generation() != claim.abort_claim_generation
        {
            return Err(HostError::Identity);
        }
        update_abort_claim(
            self, claim,
            "UPDATE capacity_reservation_storage_initialization SET abort_state = 'absent', abort_lease_until = 0 WHERE reservation_id = ?1 AND abort_state IN ('requested', 'remove_requested') AND start_state IN ('not_requested', 'requested', 'started', 'uncertain') AND result_state IN ('not_requested', 'confirmed', 'uncertain') AND remove_state IN ('not_requested', 'requested', 'removed', 'uncertain') AND (create_dispatch_state IN ('not_dispatched', 'response_received') OR (create_dispatch_state = 'outcome_unknown' AND abort_state = 'remove_requested' AND abort_helper_id IS NOT NULL))",
            "",
            "absent",
        ).await
    }

    /// Keep capacity reserved when helper absence is not known.
    pub(crate) async fn mark_storage_initializer_abort_uncertain(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        update_abort_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET abort_state = CASE WHEN abort_state = 'remove_requested' THEN 'remove_requested' ELSE 'uncertain' END, abort_lease_until = CASE WHEN abort_state = 'remove_requested' THEN abort_lease_until ELSE 0 END WHERE reservation_id = ?1 AND abort_state IN ('requested', 'remove_requested')", "", "absent").await
    }

    #[cfg(test)]
    pub(crate) async fn expire_storage_initializer_abort_lease(
        &self,
        reservation_id: i64,
    ) -> Result<(), HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            connection
                .execute(
                    "DROP TRIGGER storage_initialization_abort_lease_guard",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_storage_initialization SET abort_lease_until = 0 WHERE reservation_id = ?1 AND connection_id = ?2 AND abort_state = 'remove_requested' AND abort_lease_until > 0",
                    (reservation_id, connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "CREATE TRIGGER storage_initialization_abort_lease_guard BEFORE UPDATE OF abort_lease_until ON capacity_reservation_storage_initialization WHEN NEW.abort_lease_until != OLD.abort_lease_until AND NOT ((NEW.abort_lease_until > OLD.abort_lease_until AND NEW.abort_lease_until > 0 AND NEW.abort_claim_generation > 0) OR (NEW.abort_lease_until = 0 AND NEW.abort_state IN ('absent', 'uncertain'))) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }
}
