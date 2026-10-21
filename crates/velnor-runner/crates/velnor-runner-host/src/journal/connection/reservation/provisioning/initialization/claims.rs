impl ConnectionJournal {
    /// Record initializer identity before Docker creates the helper.
    #[cfg(test)]
    pub(crate) async fn claim_storage_initialization(
        &self,
        reservation_id: i64,
        image: &StorageInitializationImage,
        limits: crate::worker::storage_initialization::StorageInitializationLimits,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        self.claim_storage_initialization_inner(reservation_id, image, limits, None)
            .await
    }

    /// Record a layout-v2 initializer claim with its created metadata volume.
    pub(crate) async fn claim_storage_initialization_v2(
        &self,
        reservation_id: i64,
        image: &StorageInitializationImage,
        limits: crate::worker::storage_initialization::StorageInitializationLimits,
        metadata: &crate::launch::bootstrap::VerifiedCreatedStorageObject,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        self.claim_storage_initialization_inner(reservation_id, image, limits, Some(metadata))
            .await
    }

    async fn claim_storage_initialization_inner(
        &self,
        reservation_id: i64,
        image: &StorageInitializationImage,
        limits: crate::worker::storage_initialization::StorageInitializationLimits,
        metadata: Option<&crate::launch::bootstrap::VerifiedCreatedStorageObject>,
    ) -> Result<Option<StorageInitializationClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let claim = Box::pin(in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context)
                || !objects_are_ready(connection, reservation_id).await?
                || !prebind_effects_absent(connection, reservation_id).await?
                || !Self::initialization_claim_available(connection, &connection_id, reservation_id).await?
            {
                return Ok(None);
            }
            let operation_key = format!("{}-1", context.operation_id);
            Self::validate_initializer_limits(
                connection,
                reservation_id,
                &connection_id,
                &operation_key,
                image,
                limits,
            ).await?;
            let Some((home_volume, work_volume)) =
                Self::created_worker_volumes(connection, reservation_id).await?
            else {
                return Ok(None);
            };
            if home_volume == work_volume {
                return Err(HostError::Journal);
            }
            let metadata_identity = if let Some(metadata) = metadata {
                let Some(identity) = Self::metadata_identity(
                    connection,
                    reservation_id,
                    &context,
                    metadata,
                    &home_volume,
                    &work_volume,
                )
                .await?
                else {
                    return Ok(None);
                };
                Some(identity)
            } else {
                None
            };
            let helper_name = format!("velnor-init-{operation_key}");
            connection.execute(
                "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, create_state, start_state, result_state, remove_state, operation_key) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'requested', 'not_requested', 'not_requested', 'not_requested', ?17)",
                turso::params![reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), image.runtime_id(), image.source_sha(), limits.cpu_nano(), limits.memory_bytes(), limits.pids(), home_volume.as_str(), work_volume.as_str(), if metadata_identity.is_some() { 2_i64 } else { 1_i64 }, metadata_identity.as_ref().map(|value| value.0.as_str()), metadata_identity.as_ref().map(|value| value.1.as_str()), helper_name.as_str(), operation_key.as_str()],
            ).await.map_err(|_| HostError::Journal)?;
            Ok(Some(StorageInitializationClaim {
                connection_id: connection_id.clone(), reservation_id, operation_id: context.operation_id,
                generation: 1, operation_key, helper_name, engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest, image_runtime_id: image.runtime_id().to_owned(),
                image_source_sha: image.source_sha().to_owned(), home_volume, work_volume,
                layout_version: if metadata_identity.is_some() { 2 } else { 1 },
                metadata_volume: metadata_identity.as_ref().map(|value| value.0.clone()),
                metadata_volume_id: metadata_identity.as_ref().map(|value| value.1.clone()),
                nano_cpus: limits.cpu_nano(), memory_bytes: limits.memory_bytes(),
                pids_limit: limits.pids(), helper_id: None, abort_helper_id: None, abort_claim_generation: 0,
            }))
        }))
        .await?;
        #[cfg(test)]
        if claim.is_some() {
            commit_gate::pause_after_commit(reservation_id, "claim").await;
        }
        Ok(claim)
    }

    async fn initialization_claim_available(
        connection: &turso::Connection,
        connection_id: &str,
        reservation_id: i64,
    ) -> Result<bool, HostError> {
        let mut rows = connection
            .query(
                "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2",
                (reservation_id, connection_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let existing = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?;
        Ok(existing == 0)
    }

    async fn validate_initializer_limits(
        connection: &turso::Connection,
        reservation_id: i64,
        connection_id: &str,
        operation_key: &str,
        image: &StorageInitializationImage,
        limits: crate::worker::storage_initialization::StorageInitializationLimits,
    ) -> Result<(), HostError> {
        if operation_key.len() > 64
            || !valid_digest(image.runtime_id(), "sha256:", 64)
            || !valid_hex(image.source_sha(), 40)
            || limits.cpu_nano() <= 0
            || limits.memory_bytes() <= 0
            || !(1..=16).contains(&limits.pids())
        {
            return Err(HostError::Identity);
        }
        let mut reservation_rows = connection.query(
            "SELECT cpu_millicores, memory_bytes FROM capacity_reservations WHERE id = ?1 AND connection_id = ?2",
            (reservation_id, connection_id),
        ).await.map_err(|_| HostError::Journal)?;
        let reservation = reservation_rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let cpu_millicores = reservation.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let reserved_memory = reservation.get::<i64>(1).map_err(|_| HostError::Journal)?;
        if reservation_rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || limits.cpu_nano() > cpu_millicores.checked_mul(1_000_000).ok_or(HostError::Journal)?
            || limits.memory_bytes() > reserved_memory
        {
            return Err(HostError::Config);
        }
        Ok(())
    }

    async fn created_worker_volumes(
        connection: &turso::Connection,
        reservation_id: i64,
    ) -> Result<Option<(String, String)>, HostError> {
        let mut rows = connection.query(
            "SELECT role, docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND create_state = 'created' AND docker_id = docker_name AND role IN ('runner-home', 'runner-work') ORDER BY role",
            [reservation_id],
        ).await.map_err(|_| HostError::Journal)?;
        let mut home_volume = None;
        let mut work_volume = None;
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            let role = row.get::<String>(0).map_err(|_| HostError::Journal)?;
            let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
            match role.as_str() {
                "runner-home" => {
                    if home_volume.replace(name).is_some() {
                        return Err(HostError::Journal);
                    }
                }
                "runner-work" => {
                    if work_volume.replace(name).is_some() {
                        return Err(HostError::Journal);
                    }
                }
                _ => return Err(HostError::Journal),
            }
        }
        Ok(home_volume.zip(work_volume))
    }

    async fn metadata_identity(
        connection: &turso::Connection,
        reservation_id: i64,
        context: &super::HoldContext,
        metadata: &crate::launch::bootstrap::VerifiedCreatedStorageObject,
        home_volume: &str,
        work_volume: &str,
    ) -> Result<Option<(String, String)>, HostError> {
        if metadata.role() != crate::journal::StorageRole::EngineMetadata
            || metadata.reservation_id() != reservation_id
            || metadata.operation_id() != context.operation_id
            || metadata.engine_id() != context.engine_id
            || metadata.docker_root_digest() != context.docker_root_digest
            || metadata.docker_name() != metadata.docker_id()
            || metadata.docker_name() == home_volume
            || metadata.docker_name() == work_volume
        {
            return Err(HostError::Identity);
        }
        let mut rows = connection.query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata' AND create_state = 'created' AND docker_name = ?2 AND docker_id = ?3",
            (reservation_id, metadata.docker_name(), metadata.docker_id()),
        ).await.map_err(|_| HostError::Journal)?;
        let count = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_| HostError::Journal)?;
        if count != 1 {
            return Ok(None);
        }
        Ok(Some((metadata.docker_name().to_owned(), metadata.docker_id().to_owned())))
    }

    /// Record the Docker create request in the journal before Docker receives it.
    pub(crate) async fn record_storage_initializer_create_dispatched(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'dispatched' WHERE reservation_id = ?1 AND create_dispatch_state = 'not_dispatched' AND create_state = 'requested' AND helper_id IS NULL", "", "dispatch").await
    }

    /// Record that the Docker create request has a complete response.
    pub(crate) async fn record_storage_initializer_create_response_received(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        update_claim(self, claim, "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received' WHERE reservation_id = ?1 AND create_dispatch_state = 'dispatched' AND create_state = 'requested'", "", "response").await
    }

    /// Record create evidence after an interrupted initializer call.
    pub(crate) async fn resolve_storage_initializer_create_interruption(
        &self,
        claim: &StorageInitializationClaim,
        outcome: CreateInterruptionOutcome,
        response: Option<&StorageInitializerCreateResponse>,
    ) -> Result<(), HostError> {
        if self.connection_id().as_str() != claim.connection_id {
            return Err(HostError::Identity);
        }
        if let Some(response) = response
            && (!proof_matches(
                claim,
                response.reservation_id(),
                response.operation_id(),
                response.generation(),
                response.operation_key(),
                response.engine_id(),
                response.docker_root_digest(),
                response.helper_name(),
            ) || !valid_hex(response.container_id(), 64))
            {
                return Err(HostError::Identity);
            }
        let response_id = response.map(StorageInitializerCreateResponse::container_id);
        let dispatch_sql = match outcome {
            CreateInterruptionOutcome::NoRequestEntered if response_id.is_none() => {
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'not_dispatched', create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND create_dispatch_state IN ('not_dispatched', 'dispatched') AND create_state = 'requested' AND helper_id IS NULL AND ?9 IS NULL"
            }
            CreateInterruptionOutcome::RequestOutcomeUnknown if response_id.is_none() => {
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'outcome_unknown', create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND create_dispatch_state IN ('dispatched', 'outcome_unknown') AND create_state IN ('requested', 'response_recorded', 'created') AND ?9 IS NULL"
            }
            CreateInterruptionOutcome::TerminalResponseReceived => {
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', create_state = 'uncertain', helper_id = COALESCE(?9, helper_id) WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND create_dispatch_state IN ('dispatched', 'response_received', 'outcome_unknown') AND create_state IN ('requested', 'response_recorded', 'created') AND (helper_id IS NULL OR helper_id = COALESCE(?9, helper_id))"
            }
            _ => return Err(HostError::Identity),
        };
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection.execute(
                dispatch_sql,
                (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str(), response_id),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            if mark_hold_uncertain(connection, &claim.connection_id, claim.reservation_id, claim.operation_id).await.is_err() {
                let mut rows = connection.query(
                    "SELECT COUNT(*) FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id WHERE reservation.id = ?1 AND reservation.connection_id = ?2 AND intent.id = ?3 AND reservation.state = 'uncertain' AND intent.state = 'uncertain'",
                    (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id),
                ).await.map_err(|_| HostError::Journal)?;
                let count = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_| HostError::Journal)?;
                if count != 1 { return Err(HostError::Journal); }
            }
            Ok(())
        }).await?;
        #[cfg(test)]
        commit_gate::pause_after_commit(claim.reservation_id, "interruption").await;
        Ok(())
    }

    /// Keep capacity reserved when the Docker create result is unknown.
    #[cfg(test)]
    pub(crate) async fn mark_storage_initializer_create_outcome_unknown(
        &self,
        claim: &StorageInitializationClaim,
    ) -> Result<(), HostError> {
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'outcome_unknown', create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8 AND create_dispatch_state = 'dispatched' AND create_state IN ('requested', 'response_recorded')",
                (claim.reservation_id, connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &connection_id, claim.reservation_id, claim.operation_id).await
        }).await
    }
}
