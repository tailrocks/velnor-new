const INVALID_TOPOLOGY_QUERIES: &[&str] = &[
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservations AS r ON r.id = p.reservation_id LEFT JOIN connection_intents AS i ON i.id = p.operation_id WHERE r.id IS NULL OR i.id IS NULL OR i.kind != 'launch' OR i.connection_id != r.connection_id OR i.scale_set_id IS NULL OR p.engine_id != r.engine_id OR p.docker_root_digest != r.docker_root_digest OR p.sample_generation_id != r.sample_generation_id",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE t.reservation_id IS NULL OR t.topology_version NOT IN (1, 2)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_topology AS t LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = t.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p LEFT JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS s LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = s.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_roles AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_memory AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_bound_cgroups AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_publications AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS x LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = x.reservation_id WHERE p.reservation_id IS NULL",
];

const INVALID_COUNTS_QUERIES: &[&str] = &[
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles AS x WHERE x.reservation_id = p.reservation_id) != 11",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x WHERE x.reservation_id = p.reservation_id) != 9",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_memory AS x WHERE x.reservation_id = p.reservation_id) != 1",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS x WHERE x.reservation_id = p.reservation_id) != CASE t.topology_version WHEN 1 THEN 4 ELSE 5 END",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (t.topology_version = 1 AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = p.reservation_id AND object.role = 'engine-metadata'))",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = p.reservation_id AND object.role = 'engine-metadata' AND object.publication_policy_digest IS NOT NULL) != 1)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS x WHERE x.reservation_id = p.reservation_id) != 2",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS x WHERE x.reservation_id = p.reservation_id) != 1",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 0 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles WHERE reservation_id = p.reservation_id) != 0",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 0 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) != 0",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 0 AND (SELECT COUNT(*) FROM capacity_reservation_bound_cgroups WHERE reservation_id = p.reservation_id) != 0",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_roles WHERE reservation_id = p.reservation_id) != 11",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 1 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) != 14",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 1 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id AND role = 'engine-metadata') != 1",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) NOT IN (16, 18, 20)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) = 16 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id AND role = 'engine-metadata') != 3",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) = 18 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id AND role = 'engine-metadata') NOT IN (4, 5)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND t.topology_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id) = 20 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = p.reservation_id AND role = 'engine-metadata') != 6",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE s.resources_bound = 1 AND (SELECT COUNT(*) FROM capacity_reservation_bound_cgroups WHERE reservation_id = p.reservation_id) != 2",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE ((SELECT publication_state FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 'published' AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications WHERE reservation_id = p.reservation_id) != 1)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE ((SELECT publication_state FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) != 'published' AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_publications WHERE reservation_id = p.reservation_id) != 0)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE ((SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 1 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = p.reservation_id) != 1)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id WHERE ((SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = p.reservation_id) = 0 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = p.reservation_id) != 0)",
];

const INVALID_METADATA_QUERIES: &[&str] = &[
    r"SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_claims AS c LEFT JOIN capacity_reservations AS r ON r.id = c.reservation_id AND r.connection_id = c.connection_id LEFT JOIN connection_intents AS i ON i.id = c.operation_id AND i.connection_id = c.connection_id LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = c.reservation_id LEFT JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = c.reservation_id WHERE r.id IS NULL OR i.id IS NULL OR i.scale_set_id != c.scale_set_id OR p.profile_generation_id != c.profile_generation_id OR p.profile_digest != c.profile_digest OR p.engine_id != c.engine_id OR p.docker_root_digest != c.docker_root_digest OR s.instance_binding_digest != c.instance_binding_digest",
    r"SELECT COUNT(*) FROM capacity_reservation_metadata_probe_cleanups AS c LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = c.reservation_id AND claim.generation = c.generation AND claim.probe_operation_id = c.probe_operation_id LEFT JOIN resource_probe_operations AS probe ON probe.operation_id = c.probe_operation_id WHERE claim.reservation_id IS NULL OR probe.phase != 'removed' OR probe.engine_id != c.engine_id OR probe.docker_root_digest != c.docker_root_digest OR probe.operation_name != c.operation_name OR probe.container_id != c.container_id",
    r"SELECT COUNT(*) FROM capacity_reservation_metadata_settlements AS sample LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = sample.reservation_id AND claim.generation = sample.generation AND claim.probe_operation_id = sample.probe_operation_id LEFT JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = sample.reservation_id AND cleanup.generation = sample.generation WHERE claim.reservation_id IS NULL OR cleanup.reservation_id IS NULL OR sample.sample_generation_id != claim.probe_operation_id OR sample.profile_generation_id != claim.profile_generation_id OR sample.profile_digest != claim.profile_digest OR sample.engine_id != claim.engine_id OR sample.docker_root_digest != claim.docker_root_digest",
    r"SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_pools AS pool LEFT JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = pool.reservation_id AND sample.generation = pool.generation LEFT JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = pool.reservation_id AND claim.generation = pool.generation LEFT JOIN host_storage_profile_pools AS policy ON policy.generation_id = claim.profile_generation_id AND policy.pool_id = pool.pool_id WHERE sample.reservation_id IS NULL OR policy.pool_id IS NULL OR policy.medium != 'disk' OR policy.inode_policy != pool.inode_policy",
    r"SELECT COUNT(*) FROM capacity_reservation_metadata_settlements AS sample JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = sample.reservation_id AND claim.generation = sample.generation WHERE (SELECT COUNT(*) FROM capacity_reservation_metadata_settlement_pools AS pool WHERE pool.reservation_id = sample.reservation_id AND pool.generation = sample.generation) != (SELECT COUNT(*) FROM host_storage_profile_pools AS policy WHERE policy.generation_id = claim.profile_generation_id AND policy.medium = 'disk')",
    r"SELECT COUNT(*) FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = r.id WHERE r.state = 'released' AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS claim JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = claim.reservation_id AND sample.generation = claim.generation WHERE claim.reservation_id = r.id AND claim.generation = (SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims AS latest WHERE latest.reservation_id = r.id) AND sample.outcome = 'settled')",
    r"SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE (a.filesystem_id IS NULL AND (a.v16_filesystem_id_0 IS NULL OR a.v16_filesystem_id_1 IS NULL)) OR (a.filesystem_id IS NOT NULL AND (a.v16_filesystem_id_0 IS NOT NULL OR a.v16_filesystem_id_1 IS NOT NULL))",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_publications AS a WHERE (a.filesystem_id IS NULL AND (a.v16_filesystem_id_0 IS NULL OR a.v16_filesystem_id_1 IS NULL)) OR (a.filesystem_id IS NOT NULL AND (a.v16_filesystem_id_0 IS NOT NULL OR a.v16_filesystem_id_1 IS NOT NULL))",
];

const INVALID_CLEANUP_QUERIES: &[&str] = &[
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE state.storage_cleanup_proven != 1",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.runner_container_id != state.runner_container_id",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.dind_container_id != state.dind_container_id",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.instance_binding_digest != state.instance_binding_digest",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.shared_run_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'shared-run' AND create_state = 'created' AND docker_id = docker_name)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.runner_home_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'runner-home' AND create_state = 'created' AND docker_id = docker_name)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.runner_work_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'runner-work' AND create_state = 'created' AND docker_id = docker_name)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE cleanup.dind_data_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'dind-data' AND create_state = 'created' AND docker_id = docker_name)",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE (topology.topology_version = 1 AND (cleanup.metadata_volume IS NOT NULL OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id) != 4))",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE (topology.topology_version = 2 AND (cleanup.metadata_volume IS NULL OR cleanup.metadata_volume IS NOT (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND role = 'engine-metadata' AND create_state = 'created' AND docker_id = docker_name AND publication_policy_digest IS NOT NULL) OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id) != 5))",
    r"SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups AS cleanup JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = cleanup.reservation_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = cleanup.reservation_id WHERE (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = cleanup.reservation_id AND create_state = 'created' AND docker_id = docker_name) != CASE topology.topology_version WHEN 1 THEN 4 WHEN 2 THEN 5 ELSE -1 END",
];

#[cfg(test)]
#[tokio::test]
async fn query_test_split_bound_resource_checks_reject_each_invalid_count_shape()
-> Result<(), HostError> {
    let path = std::env::temp_dir().join(format!(
        "velnor-provisioning-query-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_nanos()
    ));
    let path_text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    query_test_create_tables(&connection).await?;

    for (query_index, topology, bound, roles, attachment_count, metadata_count, cgroups) in [
        (8, 1, 0, 1, 0, 0, 0),
        (9, 1, 0, 0, 1, 0, 0),
        (10, 1, 0, 0, 0, 0, 1),
        (11, 1, 1, 10, 0, 0, 2),
        (12, 1, 1, 11, 13, 0, 2),
        (13, 1, 1, 11, 14, 0, 2),
        (14, 2, 1, 11, 15, 0, 2),
        (15, 2, 1, 11, 16, 2, 2),
        (16, 2, 1, 11, 18, 3, 2),
        (18, 1, 1, 11, 14, 1, 1),
        (17, 2, 1, 11, 20, 5, 2),
        (17, 2, 1, 11, 20, 7, 2),
    ] {
        query_test_insert_shape(
            &connection,
            topology,
            bound,
            roles,
            attachment_count,
            metadata_count,
            cgroups,
        )
        .await?;
        assert_eq!(
            query_test_count(&connection, INVALID_COUNTS_QUERIES[query_index]).await?,
            1,
            "invalid count shape at query index {query_index}"
        );
    }

    query_test_insert_shape(&connection, 1, 0, 0, 0, 0, 0).await?;
    for query in INVALID_COUNTS_QUERIES.iter().take(11).skip(8) {
        assert_eq!(query_test_count(&connection, query).await?, 0);
    }

    for (topology, attachments, metadata) in
        [(1, 14, 1), (2, 16, 3), (2, 18, 4), (2, 18, 5), (2, 20, 6)]
    {
        query_test_insert_shape(&connection, topology, 1, 11, attachments, metadata, 2).await?;
        for (query_index, query) in INVALID_COUNTS_QUERIES.iter().enumerate().take(19).skip(8) {
            assert_eq!(
                query_test_count(&connection, query).await?,
                0,
                "valid count shape at query index {query_index}"
            );
        }
    }

    drop(connection);
    drop(database);
    std::fs::remove_file(path).map_err(|_| HostError::Journal)?;
    Ok(())
}

#[cfg(test)]
async fn query_test_create_tables(connection: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE capacity_reservation_provisioning (reservation_id INTEGER PRIMARY KEY)",
        "CREATE TABLE capacity_reservation_provisioning_topology (reservation_id INTEGER, topology_version INTEGER)",
        "CREATE TABLE capacity_reservation_provisioning_state (reservation_id INTEGER, resources_bound INTEGER)",
        "CREATE TABLE capacity_reservation_bound_storage_roles (reservation_id INTEGER)",
        "CREATE TABLE capacity_reservation_bound_storage_attachments (reservation_id INTEGER, role TEXT)",
        "CREATE TABLE capacity_reservation_bound_cgroups (reservation_id INTEGER)",
    ] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

#[cfg(test)]
async fn query_test_insert_shape(
    connection: &turso::Connection,
    topology: i64,
    bound: i64,
    roles: i64,
    attachments: i64,
    metadata: i64,
    cgroups: i64,
) -> Result<(), HostError> {
    connection
        .execute("DELETE FROM capacity_reservation_provisioning", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for table in [
        "capacity_reservation_provisioning_topology",
        "capacity_reservation_provisioning_state",
        "capacity_reservation_bound_storage_roles",
        "capacity_reservation_bound_storage_attachments",
        "capacity_reservation_bound_cgroups",
    ] {
        connection
            .execute(&format!("DELETE FROM {table}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning VALUES (1)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for (statement, value) in [
        (
            "INSERT INTO capacity_reservation_provisioning_topology VALUES (1, ?1)",
            topology,
        ),
        (
            "INSERT INTO capacity_reservation_provisioning_state VALUES (1, ?1)",
            bound,
        ),
    ] {
        connection
            .execute(statement, [value])
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for _ in 0..roles {
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_storage_roles VALUES (1)",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for index in 0..attachments {
        let role = if index < metadata {
            "engine-metadata"
        } else {
            "runner-root"
        };
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (1, ?1)",
                [role],
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for _ in 0..cgroups {
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_cgroups VALUES (1)",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

#[cfg(test)]
async fn query_test_count(connection: &turso::Connection, query: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
