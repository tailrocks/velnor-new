pub(super) async fn insert_claim(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    seed: &ProbeSeed,
) -> Result<StorageMetadataSettlementClaim, HostError> {
    let connection_id = scope.connection_id().as_str().to_owned();
    let instance_id = crate::journal_schema::instance_id(connection).await?;
    if instance_id != seed.instance_id {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT r.intent_id, i.scale_set_id, r.engine_id, r.docker_root_digest, r.state, r.remote_cleanup_proven, r.local_cleanup_proven, i.kind, p.profile_generation_id, p.profile_digest, p.engine_id, p.docker_root_digest, s.resources_bound, s.storage_cleanup_proven, s.jit_state, s.instance_binding_digest, c.instance_binding_digest, current.generation_id FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN capacity_reservation_provisioning_cleanups AS c ON c.reservation_id = r.id JOIN host_storage_profile_current AS current ON current.singleton = 1 WHERE r.id = ?1 AND r.connection_id = ?2",
            (reservation_id, connection_id.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let operation_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let scale_set_id = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
    let held_engine = row.get::<String>(2).map_err(|_| HostError::Journal)?;
    let held_root = row.get::<String>(3).map_err(|_| HostError::Journal)?;
    let reservation_state = row.get::<String>(4).map_err(|_| HostError::Journal)?;
    let remote_cleanup = row.get::<i64>(5).map_err(|_| HostError::Journal)?;
    let local_cleanup = row.get::<i64>(6).map_err(|_| HostError::Journal)?;
    let intent_kind = row.get::<String>(7).map_err(|_| HostError::Journal)?;
    let profile_generation_id = row.get::<String>(8).map_err(|_| HostError::Journal)?;
    let profile_digest = row.get::<String>(9).map_err(|_| HostError::Journal)?;
    let profile_engine = row.get::<String>(10).map_err(|_| HostError::Journal)?;
    let profile_root = row.get::<String>(11).map_err(|_| HostError::Journal)?;
    let resources_bound = row.get::<i64>(12).map_err(|_| HostError::Journal)?;
    let storage_cleanup = row.get::<i64>(13).map_err(|_| HostError::Journal)?;
    let jit_state = row.get::<String>(14).map_err(|_| HostError::Journal)?;
    let binding_digest = row.get::<String>(15).map_err(|_| HostError::Journal)?;
    let cleanup_binding_digest = row.get::<String>(16).map_err(|_| HostError::Journal)?;
    let current_profile_generation = row.get::<String>(17).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || reservation_state == "released"
        || remote_cleanup != 1
        || local_cleanup != 1
        || intent_kind != "launch"
        || resources_bound != 1
        || storage_cleanup != 1
        || !matches!(jit_state.as_str(), "not_requested" | "cleaned")
        || binding_digest != cleanup_binding_digest
        || current_profile_generation != profile_generation_id
        || profile_engine != held_engine
        || profile_root != held_root
        || seed.engine_id != held_engine
        || seed.docker_root_digest != held_root
    {
        return Err(HostError::Journal);
    }
    let profile = super::super::storage_budget::current_profile(connection)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.generation_id() != profile_generation_id
        || profile.profile_digest() != profile_digest
    {
        return Err(HostError::Identity);
    }
    let mut prior = connection
        .query(
            "SELECT COUNT(*), COALESCE(MAX(generation), 0) FROM capacity_reservation_metadata_settlement_claims WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let prior = prior
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let prior_count = row_u64(prior.get::<i64>(0).map_err(|_| HostError::Journal)?)?;
    let generation = prior
        .get::<i64>(1)
        .map_err(|_| HostError::Journal)?
        .checked_add(1)
        .ok_or(HostError::Journal)?;
    if generation > MAX_SETTLEMENT_GENERATION {
        return Err(HostError::Journal);
    }
    let mut unfinished = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_claims AS claim JOIN resource_probe_operations AS probe ON probe.operation_id = claim.probe_operation_id LEFT JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = claim.reservation_id AND cleanup.generation = claim.generation WHERE claim.reservation_id = ?1 AND (probe.phase NOT IN ('removed', 'aborted') OR (probe.phase = 'removed' AND cleanup.reservation_id IS NULL))",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let unfinished = unfinished
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let unfinished_count = row_u64(unfinished.get::<i64>(0).map_err(|_| HostError::Journal)?)?;
    if unfinished_count != 0
        || prior_count != u64::try_from(generation - 1).map_err(|_| HostError::Journal)?
    {
        return Err(HostError::Journal);
    }
    let claim = StorageMetadataSettlementClaim {
        connection_id,
        reservation_id,
        generation,
        operation_id,
        scale_set_id,
        instance_id,
        probe_operation_id: seed.operation_id.clone(),
        profile_generation_id,
        profile_digest,
        engine_id: held_engine,
        docker_root_digest: held_root,
        instance_binding_digest: binding_digest,
        claimed_at: Instant::now(),
    };
    connection
        .execute(
            "INSERT INTO capacity_reservation_metadata_settlement_claims (reservation_id, generation, operation_id, connection_id, scale_set_id, instance_id, probe_operation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, instance_binding_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            (claim.reservation_id, claim.generation, claim.operation_id, claim.connection_id.as_str(), claim.scale_set_id, claim.instance_id.as_str(), claim.probe_operation_id.as_str(), claim.profile_generation_id.as_str(), claim.profile_digest.as_str(), claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.instance_binding_digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(claim)
}
