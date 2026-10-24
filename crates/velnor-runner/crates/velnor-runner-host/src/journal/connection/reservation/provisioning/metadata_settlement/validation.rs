async fn validate_claim_and_cleanup(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &StorageMetadataSettlementClaim,
) -> Result<(), HostError> {
    if claim.connection_id != scope.connection_id().as_str()
        || claim.reservation_id <= 0
        || claim.generation <= 0
        || claim.operation_id <= 0
        || claim.scale_set_id <= 0
    {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT claim.instance_id, claim.profile_generation_id, claim.profile_digest, claim.engine_id, claim.docker_root_digest, claim.instance_binding_digest, probe.phase, cleanup.lease_released, sample.generation FROM capacity_reservation_metadata_settlement_claims AS claim JOIN resource_probe_operations AS probe ON probe.operation_id = claim.probe_operation_id JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = claim.reservation_id AND cleanup.generation = claim.generation AND cleanup.probe_operation_id = claim.probe_operation_id LEFT JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = claim.reservation_id AND sample.generation = claim.generation WHERE claim.reservation_id = ?1 AND claim.generation = ?2 AND claim.operation_id = ?3 AND claim.connection_id = ?4 AND claim.scale_set_id = ?5 AND claim.probe_operation_id = ?6 AND claim.profile_generation_id = ?7 AND claim.profile_digest = ?8 AND claim.engine_id = ?9 AND claim.docker_root_digest = ?10 AND claim.instance_binding_digest = ?11 AND probe.engine_id = claim.engine_id AND probe.docker_root_digest = claim.docker_root_digest AND probe.phase = 'removed' AND cleanup.engine_id = claim.engine_id AND cleanup.docker_root_digest = claim.docker_root_digest AND cleanup.phase = 'removed' AND cleanup.lease_released = 1",
            (claim.reservation_id, claim.generation, claim.operation_id, claim.connection_id.as_str(), claim.scale_set_id, claim.probe_operation_id.as_str(), claim.profile_generation_id.as_str(), claim.profile_digest.as_str(), claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.instance_binding_digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let instance_id = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    let profile_generation = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    let profile_digest = row.get::<String>(2).map_err(|_| HostError::Journal)?;
    let engine = row.get::<String>(3).map_err(|_| HostError::Journal)?;
    let root = row.get::<String>(4).map_err(|_| HostError::Journal)?;
    let binding = row.get::<String>(5).map_err(|_| HostError::Journal)?;
    let phase = row.get::<String>(6).map_err(|_| HostError::Journal)?;
    let released = row.get::<i64>(7).map_err(|_| HostError::Journal)?;
    let prior_sample = row.get::<Option<i64>>(8).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || instance_id != claim.instance_id
        || profile_generation != claim.profile_generation_id
        || profile_digest != claim.profile_digest
        || engine != claim.engine_id
        || root != claim.docker_root_digest
        || binding != claim.instance_binding_digest
        || phase != "removed"
        || released != 1
        || prior_sample.is_some()
    {
        return Err(HostError::Journal);
    }
    let linked_seed = crate::journal_schema::instance_id(connection).await?;
    if linked_seed != claim.instance_id {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn validate_reserved_pool_identities(
    connection: &turso::Connection,
    reservation_id: i64,
    sample: &StorageAdmissionSample,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, filesystem_instance_digest FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1 AND scope = 'shared-disk' AND medium = 'disk' ORDER BY pool_id",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let observed = sample
        .pools()
        .iter()
        .filter(|pool| pool.medium() == StorageMedium::Disk)
        .collect::<Vec<_>>();
    let mut seen = 0_usize;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let pool_id = row.get::<String>(0).map_err(|_| HostError::Journal)?;
        let filesystem_instance_digest = row
            .get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?;
        let sample_pool = observed
            .iter()
            .find(|pool| pool.pool_id() == pool_id)
            .ok_or(HostError::Identity)?;
        if filesystem_instance_digest.as_deref() != Some(sample_pool.filesystem_instance_digest()) {
            return Err(HostError::Identity);
        }
        seen = seen.checked_add(1).ok_or(HostError::Journal)?;
    }
    if seen != observed.len() || seen == 0 {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(in crate::journal) async fn record_probe_cleanup(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT reservation_id, generation FROM capacity_reservation_metadata_settlement_claims WHERE probe_operation_id = ?1",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(());
    };
    let reservation_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let generation = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "INSERT INTO capacity_reservation_metadata_probe_cleanups (reservation_id, generation, probe_operation_id, engine_id, docker_root_digest, operation_name, container_id, phase, lease_released) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'removed', 1)",
            (reservation_id, generation, operation_id, engine_id, docker_root_digest, operation_name, container_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn to_sql(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

fn row_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
