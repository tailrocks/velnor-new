use super::{StorageChargeTotals, V18_PUBLICATION_DDL};
use crate::error::HostError;

pub(super) async fn query_storage_charge_totals(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<StorageChargeTotals, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*), COALESCE(SUM(charge_bytes), 0), COALESCE(SUM(charge_inodes), 0), COALESCE(SUM(retained_memory_bytes), 0), (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(StorageChargeTotals {
        role_count: row.get(0).map_err(|_| HostError::Journal)?,
        charge_bytes: row.get(1).map_err(|_| HostError::Journal)?,
        charge_inodes: row.get(2).map_err(|_| HostError::Journal)?,
        retained_memory_bytes: row.get(3).map_err(|_| HostError::Journal)?,
        pool_count: row.get(4).map_err(|_| HostError::Journal)?,
    })
}

pub(super) async fn scalar_i64(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

pub(crate) async fn restore_version_eighteen_provisioning_shape(
    path: &std::path::Path,
    reservation_id: i64,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for trigger in [
        "storage_initialization_revision_insert",
        "storage_initialization_revision_update",
        "storage_initialization_revision_delete",
        "storage_initialization_insert_guard",
        "storage_initialization_identity_immutable",
        "storage_initialization_no_delete",
        "storage_initialization_create_transition_guard",
        "storage_initialization_start_transition_guard",
        "storage_initialization_result_transition_guard",
        "storage_initialization_abort_transition_guard",
        "storage_initialization_remove_transition_guard",
        "provisioning_topology_no_update",
        "provisioning_topology_no_delete",
        "provisioning_object_no_delete",
        "provisioning_object_transition",
        "bound_attachment_no_update",
        "bound_attachment_no_delete",
        "provisioning_publication_no_update",
        "provisioning_publication_no_delete",
        "provisioning_cleanup_no_update",
        "provisioning_cleanup_no_delete",
        "provisioning_storage_cleanup_guard",
        "provisioning_storage_cleanup_identity_guard",
        "provisioning_storage_cleanup_shared_run_guard",
        "provisioning_storage_cleanup_runner_home_guard",
        "provisioning_storage_cleanup_runner_work_guard",
        "provisioning_storage_cleanup_dind_data_guard",
        "provisioning_storage_cleanup_topology_v1_guard",
        "provisioning_storage_cleanup_topology_v2_guard",
        "metadata_settlement_claim_insert_guard",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "DROP TABLE IF EXISTS capacity_reservation_storage_initialization",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP INDEX IF EXISTS capacity_bound_attachment_mount", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE capacity_reservation_provisioning_topology", ())
        .await
        .map_err(|_| HostError::Journal)?;
    replace_with_version_eighteen_table(
        &connection,
        "capacity_reservation_provisioning_objects",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_objects (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('shared-run', 'runner-home', 'runner-work', 'dind-data')), docker_name TEXT NOT NULL CHECK (length(docker_name) BETWEEN 1 AND 128), docker_id TEXT CHECK (docker_id IS NULL OR docker_id = docker_name), create_state TEXT NOT NULL CHECK (create_state IN ('unclaimed', 'requested', 'created', 'uncertain')), CHECK ((create_state IN ('unclaimed', 'requested', 'uncertain') AND docker_id IS NULL) OR (create_state = 'created' AND docker_id = docker_name)), PRIMARY KEY (reservation_id, role), UNIQUE (docker_name))",
        "INSERT INTO {table} (reservation_id, role, docker_name, docker_id, create_state) SELECT reservation_id, role, docker_name, docker_id, create_state FROM {old} WHERE reservation_id = ?1 AND role != 'engine-metadata'",
        reservation_id,
    )
    .await?;
    replace_with_version_eighteen_table(
        &connection,
        "capacity_reservation_bound_storage_attachments",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_attachments (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'engine-metadata')), observer TEXT NOT NULL CHECK (observer IN ('runner', 'dind', 'probe')), container_id TEXT, mount_target TEXT NOT NULL CHECK (length(mount_target) BETWEEN 1 AND 512), source_target TEXT, read_only INTEGER NOT NULL CHECK (read_only IN (0, 1)), mount_id INTEGER NOT NULL CHECK (mount_id > 0), device_major INTEGER NOT NULL CHECK (device_major >= 0), device_minor INTEGER NOT NULL CHECK (device_minor >= 0), statx_device_major INTEGER NOT NULL CHECK (statx_device_major >= 0), statx_device_minor INTEGER NOT NULL CHECK (statx_device_minor >= 0), fs_type_magic INTEGER NOT NULL CHECK (fs_type_magic >= 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)), CHECK ((observer = 'probe' AND container_id IS NULL AND role = 'engine-metadata') OR (observer = 'runner' AND container_id IS NOT NULL) OR (observer = 'dind' AND container_id IS NOT NULL)), CHECK ((role = 'engine-metadata' AND source_target = '/var/lib/docker') OR (role != 'engine-metadata' AND source_target IS NULL)), PRIMARY KEY (reservation_id, role, observer, mount_target))",
        "INSERT INTO {table} (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest FROM {old} WHERE reservation_id = ?1 AND NOT (role = 'engine-metadata' AND observer = 'runner')",
        reservation_id,
    )
    .await?;
    replace_with_version_eighteen_table(
        &connection,
        "capacity_reservation_provisioning_publications",
        V18_PUBLICATION_DDL,
        "INSERT INTO {table} (reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only) SELECT reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only FROM {old} WHERE reservation_id = ?1",
        reservation_id,
    )
    .await?;
    replace_with_version_eighteen_table(
        &connection,
        "capacity_reservation_provisioning_cleanups",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_cleanups (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), runner_container_id TEXT NOT NULL CHECK (length(runner_container_id) BETWEEN 1 AND 256), dind_container_id TEXT NOT NULL CHECK (length(dind_container_id) BETWEEN 1 AND 256), shared_run_volume TEXT NOT NULL CHECK (length(shared_run_volume) BETWEEN 1 AND 128), runner_home_volume TEXT NOT NULL CHECK (length(runner_home_volume) BETWEEN 1 AND 128), runner_work_volume TEXT NOT NULL CHECK (length(runner_work_volume) BETWEEN 1 AND 128), dind_data_volume TEXT NOT NULL CHECK (length(dind_data_volume) BETWEEN 1 AND 128), instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'), role_mask INTEGER NOT NULL CHECK (role_mask = 2047), containers_absent INTEGER NOT NULL CHECK (containers_absent = 1), volumes_absent INTEGER NOT NULL CHECK (volumes_absent = 1), mounts_detached INTEGER NOT NULL CHECK (mounts_detached = 1), cgroups_absent INTEGER NOT NULL CHECK (cgroups_absent = 1))",
        "INSERT INTO {table} (reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent) SELECT reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent FROM {old} WHERE reservation_id = ?1",
        reservation_id,
    )
    .await?;
    connection
        .execute("PRAGMA user_version = 18", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);
    Ok(())
}

pub(super) async fn assert_version_eighteen_publication_key(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_eq!(read_user_version(&connection).await?, 18);
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_provisioning_publications'",
            (),
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
    assert_eq!(
        canonical_table_sql(&actual),
        canonical_table_sql(V18_PUBLICATION_DDL)
    );
    assert_eq!(
        publication_primary_key(&connection).await?,
        ["reservation_id"]
    );
    Ok(())
}

pub(super) async fn replace_publication_key_with_unknown_shape(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for trigger in [
        "provisioning_publication_no_update",
        "provisioning_publication_no_delete",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|error| panic!("unknown-key setup SQL failed: {error:?}"))?;
    }
    connection
        .execute(
            "ALTER TABLE capacity_reservation_provisioning_publications RENAME TO capacity_reservation_provisioning_publications_unknown",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning_publications AS SELECT * FROM capacity_reservation_provisioning_publications_unknown WHERE 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TABLE capacity_reservation_provisioning_publications_unknown",
            (),
        )
        .await
        .map_err(|error| panic!("unknown-key setup SQL failed: {error:?}"))?;
    Ok(())
}

pub(super) async fn read_user_version(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

pub(super) async fn publication_primary_key(
    connection: &turso::Connection,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(
            "PRAGMA table_info(capacity_reservation_provisioning_publications)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let ordinal = row.get::<i64>(5).map_err(|_| HostError::Journal)?;
        if ordinal > 0 {
            columns.push((
                ordinal,
                row.get::<String>(1).map_err(|_| HostError::Journal)?,
            ));
        }
    }
    columns.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    Ok(columns.into_iter().map(|(_, name)| name).collect())
}

fn canonical_table_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    if normalized.starts_with("createtableifnotexists") {
        normalized.replace_range(.."createtableifnotexists".len(), "createtable");
    }
    normalized
}

async fn replace_with_version_eighteen_table(
    connection: &turso::Connection,
    table: &str,
    create_statement: &str,
    copy_statement: &str,
    reservation_id: i64,
) -> Result<(), HostError> {
    let old = format!("{table}_c28_test");
    connection
        .execute(&format!("ALTER TABLE {table} RENAME TO {old}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(create_statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let copy_sql = copy_statement
        .replace("{table}", table)
        .replace("{old}", &old);
    connection
        .execute(&copy_sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {old}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
