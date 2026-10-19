impl ConnectionJournal {
    /// Keep the reservation charged when the claim result is not known.
    pub(crate) async fn mark_storage_initializer_claim_interrupted(
        &self,
        reservation_id: i64,
        image: &StorageInitializationImage,
        limits: StorageInitializationLimits,
    ) -> Result<bool, HostError> {
        if reservation_id <= 0
            || !valid_digest(image.runtime_id(), "sha256:", 64)
            || !valid_hex(image.source_sha(), 40)
            || limits.cpu_nano() <= 0
            || limits.memory_bytes() <= 0
            || !(1..=16).contains(&limits.pids())
        {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let found = in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Err(HostError::Journal);
            };
            if !prebind_effects_absent(connection, reservation_id).await? {
                return Err(HostError::Journal);
            }
            let mut rows = connection.query(
                "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2",
                (reservation_id, connection_id.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let count = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_| HostError::Journal)?;
            if count > 1 {
                return Err(HostError::Journal);
            }
            let found = count == 1;
            if found {
                let mut rows = connection.query(
                    "SELECT operation_id, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, create_dispatch_state, create_state, helper_id FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2",
                    (reservation_id, connection_id.as_str()),
                ).await.map_err(|_| HostError::Journal)?;
                let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
                let valid = row.get::<i64>(0).map_err(|_| HostError::Journal)? == context.operation_id
                    && row.get::<String>(1).map_err(|_| HostError::Journal)? == context.engine_id
                    && row.get::<String>(2).map_err(|_| HostError::Journal)? == context.docker_root_digest
                    && row.get::<String>(3).map_err(|_| HostError::Journal)? == image.runtime_id()
                    && row.get::<String>(4).map_err(|_| HostError::Journal)? == image.source_sha()
                    && row.get::<i64>(5).map_err(|_| HostError::Journal)? == limits.cpu_nano()
                    && row.get::<i64>(6).map_err(|_| HostError::Journal)? == limits.memory_bytes()
                    && row.get::<i64>(7).map_err(|_| HostError::Journal)? == limits.pids()
                    && row.get::<String>(8).map_err(|_| HostError::Journal)? == "not_dispatched"
                    && matches!(row.get::<String>(9).map_err(|_| HostError::Journal)?.as_str(), "requested" | "uncertain")
                    && row.get::<Option<String>>(10).map_err(|_| HostError::Journal)?.is_none();
                if !valid || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                    return Err(HostError::Identity);
                }
                if context.reservation_state == "reserved" && context.intent_state == "pending" {
                    let changed = connection.execute(
                        "UPDATE capacity_reservation_storage_initialization SET create_state = 'uncertain' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND create_dispatch_state = 'not_dispatched' AND create_state = 'requested' AND helper_id IS NULL",
                        (reservation_id, connection_id.as_str(), context.operation_id),
                    ).await.map_err(|_| HostError::Journal)?;
                    if changed != 1 { return Err(HostError::Journal); }
                }
            }
            mark_hold_uncertain_idempotent(connection, &connection_id, reservation_id, context.operation_id).await?;
            Ok(found)
        }).await?;
        #[cfg(test)]
        commit_gate::pause_after_commit(reservation_id, "claim-interrupted").await;
        Ok(found)
    }
}

async fn mark_hold_uncertain_idempotent(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    operation_id: i64,
) -> Result<(), HostError> {
    if mark_hold_uncertain(connection, connection_id, reservation_id, operation_id)
        .await
        .is_ok()
    {
        return Ok(());
    }
    let mut rows = connection.query(
        "SELECT COUNT(*) FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id WHERE reservation.id = ?1 AND reservation.connection_id = ?2 AND intent.id = ?3 AND reservation.state = 'uncertain' AND intent.state = 'uncertain'",
        (reservation_id, connection_id, operation_id),
    ).await.map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
