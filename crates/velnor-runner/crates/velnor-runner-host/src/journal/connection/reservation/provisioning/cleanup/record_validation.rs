use crate::error::HostError;

pub(super) async fn validate_existing_cleanup(
    connection: &turso::Connection,
    reservation_id: i64,
    runner_id: &str,
    dind_id: &str,
    volumes: &[String],
    binding_digest: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, metadata_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let mut recorded_volumes = vec![
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        row.get::<String>(3).map_err(|_| HostError::Journal)?,
        row.get::<String>(4).map_err(|_| HostError::Journal)?,
        row.get::<String>(5).map_err(|_| HostError::Journal)?,
    ];
    let metadata_volume = row
        .get::<Option<String>>(6)
        .map_err(|_| HostError::Journal)?;
    if let Some(metadata_volume) = metadata_volume {
        recorded_volumes.push(metadata_volume);
    }
    if row.get::<String>(0).map_err(|_| HostError::Journal)? != runner_id
        || row.get::<String>(1).map_err(|_| HostError::Journal)? != dind_id
        || recorded_volumes.as_slice() != volumes
        || row.get::<String>(7).map_err(|_| HostError::Journal)? != binding_digest
        || row.get::<i64>(8).map_err(|_| HostError::Journal)? != 2047
        || (9..13).any(|index| row.get::<i64>(index).ok() != Some(1))
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) async fn storage_cleanup_already_proven(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    Ok(row.get::<i64>(0).map_err(|_| HostError::Journal)? == 1)
}
