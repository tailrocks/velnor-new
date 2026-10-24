pub(super) async fn record(
    scope: &ConnectionJournal,
    claim: &StorageMetadataSettlementClaim,
    sample: StorageAdmissionSample,
) -> Result<MetadataSettlementResult, HostError> {
    let sample_identity = sample.identity();
    if sample_identity.sample_generation_id() != claim.probe_operation_id
        || sample_identity.profile_generation_id() != claim.profile_generation_id
        || sample_identity.profile_digest() != claim.profile_digest
        || sample_identity.engine_id() != claim.engine_id
        || sample_identity.docker_root_digest() != claim.docker_root_digest
    {
        return Err(HostError::Identity);
    }
    if sample
        .observed_at()
        .checked_duration_since(claim.claimed_at)
        .is_none()
    {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        record_in_transaction(connection, scope, claim, &sample).await
    })
    .await
}

async fn record_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &StorageMetadataSettlementClaim,
    sample: &StorageAdmissionSample,
) -> Result<MetadataSettlementResult, HostError> {
    scope.validate_scope(connection).await?;
    validate_claim_and_cleanup(connection, scope, claim).await?;
    let profile = super::super::storage_budget::current_profile(connection)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.generation_id() != claim.profile_generation_id
        || profile.profile_digest() != claim.profile_digest
    {
        return Err(HostError::Identity);
    }
    sample.validate_metadata_settlement_for(
        &profile,
        &claim.probe_operation_id,
        &claim.engine_id,
        &claim.docker_root_digest,
        Instant::now(),
    )?;
    validate_reserved_pool_identities(connection, claim.reservation_id, sample).await?;
    let fits =
        admission::metadata_capacity_fits(connection, claim.reservation_id, &profile, sample)
            .await?;
    let outcome = if fits { "settled" } else { "held_at_capacity" };
    insert_sample_settlement(connection, claim, sample, &profile, outcome).await?;
    if !fits {
        return Ok(MetadataSettlementResult::HeldAtCapacity);
    }
    if crate::journal::connection::reservation::has_conflict_recovery_claim(
        connection,
        claim.reservation_id,
    )
    .await?
    {
        if !crate::journal::connection::reservation::record_conflict_metadata_settlement(
            connection,
            claim.reservation_id,
            claim.generation,
        )
        .await?
        {
            return Err(HostError::Identity);
        }
        return Ok(MetadataSettlementResult::Settled);
    }
    if !super::release_ready(connection, claim.reservation_id).await? {
        return Err(HostError::Journal);
    }
    super::cleanup::prove_cleanup_before_release(
        connection,
        claim.connection_id.as_str(),
        claim.operation_id,
        claim.reservation_id,
        true,
    )
    .await?;
    Ok(MetadataSettlementResult::Settled)
}

async fn insert_sample_settlement(
    connection: &turso::Connection,
    claim: &StorageMetadataSettlementClaim,
    sample: &StorageAdmissionSample,
    profile: &super::super::storage_budget::StorageProfile,
    outcome: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_metadata_settlements (reservation_id, generation, probe_operation_id, sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms, outcome) VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (claim.reservation_id, claim.generation, claim.probe_operation_id.as_str(), claim.profile_generation_id.as_str(), claim.profile_digest.as_str(), claim.engine_id.as_str(), claim.docker_root_digest.as_str(), to_sql(sample.observed_at_unix_ms())?, outcome),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for pool in sample
        .pools()
        .iter()
        .filter(|pool| pool.medium() == StorageMedium::Disk)
    {
        connection
            .execute(
                "INSERT INTO capacity_reservation_metadata_settlement_pools (reservation_id, generation, pool_id, filesystem_instance_digest, medium, filesystem_kind, inode_policy, total_bytes, available_bytes, total_inodes, available_inodes) VALUES (?1, ?2, ?3, ?4, 'disk', ?5, ?6, ?7, ?8, ?9, ?10)",
                (claim.reservation_id, claim.generation, pool.pool_id(), pool.filesystem_instance_digest(), pool.filesystem_kind().as_str(), profile.pool_limit(pool.pool_id())?.inode_policy().as_str(), to_sql(pool.total_bytes())?, to_sql(pool.available_bytes())?, to_sql(pool.total_inodes())?, to_sql(pool.available_inodes())?),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
