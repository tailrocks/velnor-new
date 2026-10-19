pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, statement) in TABLES.iter().chain(INDEXES.iter()).chain(TRIGGERS.iter()) {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in TABLES.iter().chain(INDEXES.iter()).chain(TRIGGERS.iter()) {
        let kind = object_kind(name);
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, *name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    validate_rows(connection).await
}

fn object_kind(name: &str) -> &'static str {
    if TABLES.iter().any(|entry| entry.0 == name) {
        "table"
    } else if INDEXES.iter().any(|entry| entry.0 == name) {
        "index"
    } else {
        "trigger"
    }
}

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid_topology = scalar_count(
        connection,
        "SELECT (SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservations AS r ON r.id = p.reservation_id LEFT JOIN connection_intents AS i ON i.id = p.operation_id WHERE r.id IS NULL OR i.id IS NULL OR i.kind != 'launch' OR i.connection_id != r.connection_id OR i.scale_set_id IS NULL OR p.engine_id != r.engine_id OR p.docker_root_digest != r.docker_root_digest OR p.sample_generation_id != r.sample_generation_id) + (SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE t.reservation_id IS NULL OR t.topology_version NOT IN (1, 2)) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_topology AS t LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = t.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS s LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = s.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_memory AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_bound_cgroups AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL)",
    )
    .await?;
    if invalid_topology != 0 {
        return Err(HostError::Journal);
    }
    let invalid_counts = scalar_count(
        connection,
        "SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles AS x WHERE x.reservation_id = p.reservation_id) != 11 OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x WHERE x.reservation_id = p.reservation_id) != 9 OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_memory AS x WHERE x.reservation_id = p.reservation_id) != 1 OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS x WHERE x.reservation_id = p.reservation_id) != CASE t.topology_version WHEN 1 THEN 4 ELSE 5 END OR (t.topology_version = 1 AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = p.reservation_id AND object.role = 'engine-metadata')) OR (t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = p.reservation_id AND object.role = 'engine-metadata' AND object.publication_policy_digest IS NOT NULL) != 1) OR (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS x WHERE x.reservation_id = p.reservation_id) != 2 OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS x WHERE x.reservation_id = p.reservation_id) != 1 OR ((SELECT resources_bound FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 0 AND ((SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles WHERE reservation_id = p.reservation_id) != 0 OR (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) != 0 OR (SELECT COUNT(*) FROM capacity_reservation_bound_cgroups WHERE reservation_id = p.reservation_id) != 0)) OR ((SELECT resources_bound FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 1 AND ((SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles WHERE reservation_id = p.reservation_id) != 11 OR (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) != CASE t.topology_version WHEN 1 THEN 14 ELSE 16 END OR (SELECT COUNT(*) FROM capacity_reservation_bound_cgroups WHERE reservation_id = p.reservation_id) != 2 OR (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id AND role = 'engine-metadata') != CASE t.topology_version WHEN 1 THEN 1 ELSE 3 END)) OR ((SELECT publication_state FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 'published' AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications WHERE reservation_id = p.reservation_id) != 1) OR ((SELECT publication_state FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) != 'published' AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications WHERE reservation_id = p.reservation_id) != 0) OR ((SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 1 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = p.reservation_id) != 1) OR ((SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 0 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = p.reservation_id) != 0)",
    )
    .await?;
    if invalid_counts != 0 {
        return Err(HostError::Journal);
    }
    validate_cleanup_rows(connection).await?;
    let invalid_metadata = scalar_count(
        connection,
        "SELECT (SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_claims AS c LEFT JOIN capacity_reservations AS r ON r.id = c.reservation_id AND r.connection_id = c.connection_id LEFT JOIN connection_intents AS i ON i.id = c.operation_id AND i.connection_id = c.connection_id LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = c.reservation_id LEFT JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = c.reservation_id WHERE r.id IS NULL OR i.id IS NULL OR i.scale_set_id != c.scale_set_id OR p.profile_generation_id != c.profile_generation_id OR p.profile_digest != c.profile_digest OR p.engine_id != c.engine_id OR p.docker_root_digest != c.docker_root_digest OR s.instance_binding_digest != c.instance_binding_digest) + (SELECT COUNT(*) FROM capacity_reservation_metadata_probe_cleanups AS c LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = c.reservation_id AND claim.generation = c.generation AND claim.probe_operation_id = c.probe_operation_id LEFT JOIN resource_probe_operations AS probe ON probe.operation_id = c.probe_operation_id WHERE claim.reservation_id IS NULL OR probe.phase != 'removed' OR probe.engine_id != c.engine_id OR probe.docker_root_digest != c.docker_root_digest OR probe.operation_name != c.operation_name OR probe.container_id != c.container_id) + (SELECT COUNT(*) FROM capacity_reservation_metadata_settlements AS sample LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = sample.reservation_id AND claim.generation = sample.generation AND claim.probe_operation_id = sample.probe_operation_id LEFT JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = sample.reservation_id AND cleanup.generation = sample.generation WHERE claim.reservation_id IS NULL OR cleanup.reservation_id IS NULL OR sample.sample_generation_id != claim.probe_operation_id OR sample.profile_generation_id != claim.profile_generation_id OR sample.profile_digest != claim.profile_digest OR sample.engine_id != claim.engine_id OR sample.docker_root_digest != claim.docker_root_digest) + (SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_pools AS pool LEFT JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = pool.reservation_id AND sample.generation = pool.generation LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = pool.reservation_id AND claim.generation = pool.generation LEFT JOIN host_storage_profile_pools AS policy ON policy.generation_id = claim.profile_generation_id AND policy.pool_id = pool.pool_id WHERE sample.reservation_id IS NULL OR policy.pool_id IS NULL OR policy.medium != 'disk' OR policy.inode_policy != pool.inode_policy) + (SELECT COUNT(*) FROM capacity_reservation_metadata_settlements AS sample JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = sample.reservation_id AND claim.generation = sample.generation WHERE (SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_pools AS pool WHERE pool.reservation_id = sample.reservation_id AND pool.generation = sample.generation) != (SELECT COUNT(*) FROM host_storage_profile_pools AS policy WHERE policy.generation_id = claim.profile_generation_id AND policy.medium = 'disk')) + (SELECT COUNT(*) FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = r.id WHERE r.state = 'released' AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS claim JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = claim.reservation_id AND sample.generation = claim.generation WHERE claim.reservation_id = r.id AND claim.generation = (SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims AS latest WHERE latest.reservation_id = r.id) AND sample.outcome = 'settled')) + (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE (a.filesystem_id IS NULL AND (a.v16_filesystem_id_0 IS NULL OR a.v16_filesystem_id_1 IS NULL)) OR (a.filesystem_id IS NOT NULL AND (a.v16_filesystem_id_0 IS NOT NULL OR a.v16_filesystem_id_1 IS NOT NULL))) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications AS a WHERE (a.filesystem_id IS NULL AND (a.v16_filesystem_id_0 IS NULL OR a.v16_filesystem_id_1 IS NULL)) OR (a.filesystem_id IS NOT NULL AND (a.v16_filesystem_id_0 IS NOT NULL OR a.v16_filesystem_id_1 IS NOT NULL)))",
    )
    .await?;
    if invalid_metadata != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_cleanup_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = scalar_count(
        connection,
        "SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE state.storage_cleanup_proven != 1 OR cleanup.runner_container_id != state.runner_container_id OR cleanup.dind_container_id != state.dind_container_id OR cleanup.instance_binding_digest != state.instance_binding_digest OR cleanup.shared_run_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'shared-run' AND create_state = 'created' AND docker_id = docker_name) OR cleanup.runner_home_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'runner-home' AND create_state = 'created' AND docker_id = docker_name) OR cleanup.runner_work_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'runner-work' AND create_state = 'created' AND docker_id = docker_name) OR cleanup.dind_data_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'dind-data' AND create_state = 'created' AND docker_id = docker_name) OR (topology.topology_version = 1 AND (cleanup.metadata_volume IS NOT NULL OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id) != 4)) OR (topology.topology_version = 2 AND (cleanup.metadata_volume IS NULL OR cleanup.metadata_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'engine-metadata' AND create_state = 'created' AND docker_id = docker_name AND publication_policy_digest IS NOT NULL) OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id) != 5)) OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND create_state = 'created' AND docker_id = docker_name) != CASE topology.topology_version WHEN 1 THEN 4 WHEN 2 THEN 5 ELSE -1 END",
    )
    .await?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

fn normalize(sql: &str) -> String {
    sql.to_ascii_lowercase()
        .replace("create table if not exists", "create table")
        .replace("create index if not exists", "create index")
        .replace("create trigger if not exists", "create trigger")
        .replace("left outer join", "left join")
        .replace(char::is_whitespace, "")
}
