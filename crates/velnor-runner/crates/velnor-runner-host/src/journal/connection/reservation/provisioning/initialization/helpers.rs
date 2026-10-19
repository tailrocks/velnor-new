async fn update_abort_claim(
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    sql: &str,
    value: &str,
    stage: &'static str,
) -> Result<(), HostError> {
    #[cfg(not(test))]
    let _ = stage;
    if journal.connection_id().as_str() != claim.connection_id || claim.abort_claim_generation <= 0
    {
        return Err(HostError::Identity);
    }
    let now = unix_seconds()?;
    let statement = format!(
        "{sql} AND connection_id = ?3 AND abort_claim_generation = ?4 AND abort_lease_until > ?5"
    );
    let changed = in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        connection
            .execute(
                &statement,
                (
                    claim.reservation_id,
                    value,
                    claim.connection_id.as_str(),
                    claim.abort_claim_generation,
                    now,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)
    })
    .await?;
    if changed == 1 {
        #[cfg(test)]
        commit_gate::pause_after_commit(claim.reservation_id, stage).await;
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}

async fn update_claim(
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    sql: &str,
    value: &str,
    stage: &'static str,
) -> Result<(), HostError> {
    #[cfg(not(test))]
    let _ = stage;
    if journal.connection_id().as_str() != claim.connection_id {
        return Err(HostError::Identity);
    }
    Box::pin(in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        let changed = if sql.contains("?2") {
            let statement = format!("{sql} AND connection_id = ?3 AND operation_id = ?4 AND generation = ?5 AND engine_id = ?6 AND docker_root_digest = ?7 AND operation_key = ?8 AND helper_name = ?9");
            connection.execute(&statement, (claim.reservation_id, value, claim.connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str())).await.map_err(|_| HostError::Journal)?
        } else {
            let statement = format!("{sql} AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8");
            connection.execute(&statement, (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str())).await.map_err(|_| HostError::Journal)?
        };
        if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
    }))
    .await?;
    #[cfg(test)]
    commit_gate::pause_after_commit(claim.reservation_id, stage).await;
    Ok(())
}

async fn mark_uncertain(
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    sql: &str,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id {
        return Err(HostError::Identity);
    }
    in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        let statement = format!("{sql} AND connection_id = ?2 AND operation_id = ?3 AND generation = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND operation_key = ?7 AND helper_name = ?8");
        let changed = connection.execute(
            &statement,
            (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str()),
        ).await.map_err(|_| HostError::Journal)?;
        if changed == 1
            && mark_hold_uncertain(
                connection,
                &claim.connection_id,
                claim.reservation_id,
                claim.operation_id,
            )
            .await
            .is_ok()
        {
            return Ok(());
        }
        let mut rows = connection.query(
            "SELECT COUNT(*) FROM capacity_reservation_storage_initialization AS init JOIN capacity_reservations AS reservation ON reservation.id = init.reservation_id JOIN connection_intents AS intent ON intent.id = init.operation_id WHERE init.reservation_id = ?1 AND init.connection_id = ?2 AND init.operation_id = ?3 AND init.generation = ?4 AND init.engine_id = ?5 AND init.docker_root_digest = ?6 AND init.operation_key = ?7 AND init.helper_name = ?8 AND reservation.state = 'uncertain' AND intent.state = 'uncertain'",
            (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.generation, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.operation_key.as_str(), claim.helper_name.as_str()),
        ).await.map_err(|_| HostError::Journal)?;
        let already_uncertain = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?
            == 1;
        if already_uncertain {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    }).await
}

fn proof_matches(
    claim: &StorageInitializationClaim,
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: &str,
    engine_id: &str,
    docker_root_digest: &str,
    helper_name: &str,
) -> bool {
    reservation_id == claim.reservation_id
        && operation_id == claim.operation_id
        && generation == claim.generation
        && operation_key == claim.operation_key
        && engine_id == claim.engine_id
        && docker_root_digest == claim.docker_root_digest
        && helper_name == claim.helper_name
}

pub(super) async fn ready_for_job_create(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection.query(
        "SELECT COUNT(*) FROM capacity_reservation_storage_initialization AS init JOIN capacity_reservation_storage_handoffs AS handoff ON handoff.reservation_id = init.reservation_id AND handoff.state = 'ready_held' WHERE init.reservation_id = ?1 AND init.layout_version = 2 AND init.create_state = 'created' AND init.start_state = 'started' AND init.result_state = 'confirmed' AND init.remove_state = 'not_requested' AND init.helper_id IS NOT NULL AND init.metadata_layout_digest IS NOT NULL AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = init.reservation_id AND object.role = 'engine-metadata' AND object.create_state = 'created' AND object.docker_name = init.metadata_volume AND object.docker_id = init.metadata_volume_id)",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

async fn objects_are_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection.query(
        "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND create_state = 'created' AND docker_id = docker_name AND role IN ('runner-home', 'runner-work')",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 2)
}

async fn prebind_effects_absent(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = ?1 AND state.bootstrap_start_state = 'not_requested' AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS container WHERE container.reservation_id = state.reservation_id AND (container.create_state != 'unclaimed' OR container.start_state != 'not_requested'))",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

fn valid_digest(value: &str, prefix: &str, digits: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|digest| valid_hex(digest, digits))
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
