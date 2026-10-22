/// Verified readiness for the version two storage layout.
pub(crate) struct VerifiedMetadataInitialization {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    operation_key: String,
    layout_digest: String,
}

/// Permission to attach workers while the initializer holds its tmpfs mounts.
pub(crate) struct VerifiedMetadataInitializationHandoff {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    helper_id: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    operation_key: String,
    layout_digest: String,
    ready_digest: String,
}

impl VerifiedMetadataInitializationHandoff {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn helper_name(&self) -> &str {
        &self.helper_name
    }
    pub(crate) fn helper_id(&self) -> &str {
        &self.helper_id
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
    pub(crate) fn ready_digest(&self) -> &str {
        &self.ready_digest
    }
}

impl VerifiedMetadataInitialization {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    #[expect(
        clippy::unused_self,
        reason = "The receiver carries verified version-two metadata readiness; callers must hold that witness."
    )]
    pub(crate) const fn layout_version(&self) -> i64 {
        2
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
}

impl ConnectionJournal {
    /// Record a verified READY result while the initializer still holds its mounts.
    pub(crate) async fn record_storage_initializer_ready_held(
        &self,
        claim: &StorageInitializationClaim,
        proof: &crate::worker::storage_initialization::VerifiedStorageInitializerAwaitingHandoff,
    ) -> Result<VerifiedMetadataInitializationHandoff, HostError> {
        let connection_id = self.connection_id().as_str().to_owned();
        if proof.connection_id() != connection_id
            || proof.reservation_id() != claim.reservation_id()
            || proof.operation_id() != claim.operation_id()
            || proof.generation() != claim.generation()
            || proof.operation_key() != claim.operation_key()
            || proof.engine_id() != claim.engine_id()
            || proof.docker_root_digest() != claim.docker_root_digest()
            || proof.helper_name() != claim.helper_name()
            || Some(proof.container_id()) != claim.helper_id()
            || proof.layout_version() != 2
            || claim.layout_version() != 2
            || Some(proof.metadata_volume_name()) != claim.metadata_volume_name()
            || Some(proof.metadata_volume_id()) != claim.metadata_volume_id()
            || !valid_digest(proof.ready_digest(), "", 64)
        {
            return Err(HostError::Identity);
        }
        crate::journal::connection::storage::in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            connection.execute(
                "INSERT INTO capacity_reservation_storage_handoffs (reservation_id, connection_id, operation_id, generation, helper_name, helper_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, operation_key, layout_digest, ready_digest, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'ready_held') ON CONFLICT(reservation_id) DO NOTHING",
                (proof.reservation_id(), proof.connection_id(), proof.operation_id(), proof.generation(), proof.helper_name(), proof.container_id(), proof.engine_id(), proof.docker_root_digest(), proof.metadata_volume_name(), proof.metadata_volume_id(), proof.operation_key(), proof.layout_digest(), proof.ready_digest()),
            ).await.map_err(|_| HostError::Journal)?;
            let mut rows = connection.query(
                "SELECT COUNT(*) FROM capacity_reservation_storage_handoffs WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND helper_name = ?5 AND helper_id = ?6 AND engine_id = ?7 AND docker_root_digest = ?8 AND metadata_volume_name = ?9 AND metadata_volume_id = ?10 AND operation_key = ?11 AND layout_digest = ?12 AND ready_digest = ?13 AND state IN ('ready_held', 'attached')",
                (proof.reservation_id(), proof.connection_id(), proof.operation_id(), proof.generation(), proof.helper_name(), proof.container_id(), proof.engine_id(), proof.docker_root_digest(), proof.metadata_volume_name(), proof.metadata_volume_id(), proof.operation_key(), proof.layout_digest(), proof.ready_digest()),
            ).await.map_err(|_| HostError::Journal)?;
            let exact = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_| HostError::Journal)?;
            if exact != 1 { return Err(HostError::Identity); }
            Ok(())
        }).await?;
        Ok(VerifiedMetadataInitializationHandoff {
            connection_id,
            reservation_id: proof.reservation_id(),
            operation_id: proof.operation_id(),
            generation: proof.generation(),
            engine_id: proof.engine_id().to_owned(),
            docker_root_digest: proof.docker_root_digest().to_owned(),
            helper_name: proof.helper_name().to_owned(),
            helper_id: proof.container_id().to_owned(),
            metadata_volume_name: proof.metadata_volume_name().to_owned(),
            metadata_volume_id: proof.metadata_volume_id().to_owned(),
            operation_key: proof.operation_key().to_owned(),
            layout_digest: proof.layout_digest().to_owned(),
            ready_digest: proof.ready_digest().to_owned(),
        })
    }

    /// Record the inspected worker mounts before the host releases the initializer.
    pub(crate) async fn record_storage_initializer_pair_mounted(
        &self,
        held: &VerifiedMetadataInitializationHandoff,
        proof: &crate::launch::bootstrap::VerifiedBootstrapMountHandoff,
    ) -> Result<(), HostError> {
        if held.connection_id() != self.connection_id().as_str()
            || proof.connection_id() != held.connection_id()
            || proof.reservation_id() != held.reservation_id()
            || proof.operation_id() != held.operation_id()
            || proof.engine_id() != held.engine_id()
            || proof.docker_root_digest() != held.docker_root_digest()
            || proof.metadata_volume_name() != held.metadata_volume_name()
            || proof.metadata_volume_id() != held.metadata_volume_id()
            || proof.layout_digest() != held.layout_digest()
            || proof.runner_container_id() == proof.dind_container_id()
            || !valid_digest(proof.mount_evidence_digest(), "", 64)
        {
            return Err(HostError::Identity);
        }
        crate::journal::connection::storage::in_transaction(
            self.journal_ref(),
            async |connection| {
                self.validate_scope(connection).await?;
                let changed = connection
                    .execute(
                        "UPDATE capacity_reservation_storage_handoffs SET state = 'attached', mount_evidence_digest = ?1, runner_container_id = ?2, dind_container_id = ?3 WHERE reservation_id = ?4 AND connection_id = ?5 AND operation_id = ?6 AND generation = ?7 AND helper_name = ?8 AND helper_id = ?9 AND engine_id = ?10 AND docker_root_digest = ?11 AND metadata_volume_name = ?12 AND metadata_volume_id = ?13 AND operation_key = ?14 AND layout_digest = ?15 AND ready_digest = ?16 AND state = 'ready_held'",
                        (proof.mount_evidence_digest(), proof.runner_container_id(), proof.dind_container_id(), held.reservation_id(), held.connection_id(), held.operation_id(), held.generation(), held.helper_name(), held.helper_id(), held.engine_id(), held.docker_root_digest(), held.metadata_volume_name(), held.metadata_volume_id(), held.operation_key(), held.layout_digest(), held.ready_digest()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if changed != 1 {
                    let mut rows = connection.query(
                        "SELECT COUNT(*) FROM capacity_reservation_storage_handoffs WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND helper_name = ?5 AND helper_id = ?6 AND engine_id = ?7 AND docker_root_digest = ?8 AND metadata_volume_name = ?9 AND metadata_volume_id = ?10 AND operation_key = ?11 AND layout_digest = ?12 AND ready_digest = ?13 AND state = 'attached' AND mount_evidence_digest = ?14 AND runner_container_id = ?15 AND dind_container_id = ?16",
                        (held.reservation_id(), held.connection_id(), held.operation_id(), held.generation(), held.helper_name(), held.helper_id(), held.engine_id(), held.docker_root_digest(), held.metadata_volume_name(), held.metadata_volume_id(), held.operation_key(), held.layout_digest(), held.ready_digest(), proof.mount_evidence_digest(), proof.runner_container_id(), proof.dind_container_id()),
                    ).await.map_err(|_| HostError::Journal)?;
                    let exact = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_| HostError::Journal)?;
                    if exact != 1 { return Err(HostError::Identity); }
                }
                Ok(())
            },
        )
        .await
    }

    /// Return metadata readiness only for an exact confirmed v2 result.
    pub(crate) async fn metadata_initialization_ready(
        &self,
        reservation_id: i64,
    ) -> Result<Option<VerifiedMetadataInitialization>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        crate::journal::connection::storage::in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = super::hold_context(connection, &connection_id, reservation_id).await? else { return Ok(None); };
            if !super::hold_is_claimable(&context) { return Ok(None); }
            let mut rows = connection.query(
                "SELECT init.operation_id, init.engine_id, init.docker_root_digest, init.metadata_volume, init.metadata_volume_id, init.operation_key, init.metadata_layout_digest, object.docker_name, object.docker_id, handoff.ready_digest FROM capacity_reservation_storage_initialization AS init JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = init.reservation_id AND object.role = 'engine-metadata' AND object.create_state = 'created' JOIN capacity_reservation_storage_handoffs AS handoff ON handoff.reservation_id = init.reservation_id AND handoff.state = 'attached' WHERE init.reservation_id = ?1 AND init.connection_id = ?2 AND handoff.connection_id = init.connection_id AND handoff.operation_id = init.operation_id AND handoff.engine_id = init.engine_id AND handoff.docker_root_digest = init.docker_root_digest AND handoff.metadata_volume_name = init.metadata_volume AND handoff.metadata_volume_id = init.metadata_volume_id AND handoff.layout_digest = init.metadata_layout_digest AND init.layout_version = 2 AND init.create_state = 'created' AND init.start_state = 'started' AND init.result_state = 'confirmed' AND init.remove_state = 'removed' AND init.helper_id IS NOT NULL AND init.metadata_layout_digest IS NOT NULL AND object.docker_name = init.metadata_volume AND object.docker_id = init.metadata_volume_id",
                (reservation_id, connection_id.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None); };
            let receipt = VerifiedMetadataInitialization {
                connection_id: connection_id.clone(), reservation_id,
                operation_id: row.get(0).map_err(|_| HostError::Journal)?,
                engine_id: row.get(1).map_err(|_| HostError::Journal)?,
                docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
                metadata_volume_name: row.get(3).map_err(|_| HostError::Journal)?,
                metadata_volume_id: row.get(4).map_err(|_| HostError::Journal)?,
                operation_key: row.get(5).map_err(|_| HostError::Journal)?,
                layout_digest: row.get(6).map_err(|_| HostError::Journal)?,
            };
            if row.get::<String>(7).map_err(|_| HostError::Journal)? != receipt.metadata_volume_name
                || row.get::<String>(8).map_err(|_| HostError::Journal)? != receipt.metadata_volume_id
                || receipt.operation_id != context.operation_id || receipt.engine_id != context.engine_id
                || receipt.docker_root_digest != context.docker_root_digest || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            { return Err(HostError::Journal); }
            Ok(Some(receipt))
        }).await
    }
}
