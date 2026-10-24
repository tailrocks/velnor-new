/// Add a metadata volume record for new capacity reservations. Keep earlier reservation rows unchanged.
// Exact v18 publication table from the frozen v18 DDL source.
// See C27 schema/provisioning/ddl.rs, SHA256 ccc94ccde7c4dca5da099e46f52361873d9d7b27f76bd1005649f670a890e29c.
const VERSION_EIGHTEEN_PUBLICATION_DDL: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_publications (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), runner_id INTEGER NOT NULL CHECK (runner_id > 0), runner_name TEXT NOT NULL CHECK (length(runner_name) BETWEEN 1 AND 128), runner_scale_set_id INTEGER NOT NULL CHECK (runner_scale_set_id > 0), target TEXT NOT NULL CHECK (target = '/opt/velnor/job-facts/storage-cache-admission.json'), projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64 AND projection_sha256 NOT GLOB '*[^0-9a-f]*'), directory_device_major INTEGER NOT NULL CHECK (directory_device_major >= 0), directory_device_minor INTEGER NOT NULL CHECK (directory_device_minor >= 0), directory_inode INTEGER NOT NULL CHECK (directory_inode > 0), mount_id INTEGER NOT NULL CHECK (mount_id > 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), read_only INTEGER NOT NULL CHECK (read_only = 1), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)))";
pub(super) async fn upgrade_from_version_eighteen(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    super::storage_budget::bootstrap(connection).await?;
    create_topology_table(connection).await?;
    // This guard uses the cleanup table that the migration replaces. Before ALTER TABLE changes
    // its table reference, remove this guard. Schema initialization installs the current guard again.
    connection
        .execute(
            "DROP TRIGGER IF EXISTS metadata_settlement_claim_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    upgrade_objects(connection).await?;
    upgrade_v18_attachments(connection).await?;
    upgrade_publications(connection).await?;
    upgrade_cleanups(connection).await?;
    mark_existing_topology_legacy(connection).await?;
    bootstrap(connection).await?;
    Ok(())
}

async fn create_topology_table(connection: &turso::Connection) -> Result<(), HostError> {
    let statement = TABLES
        .iter()
        .find(|(name, _)| *name == "capacity_reservation_provisioning_topology")
        .map(|(_, statement)| *statement)
        .ok_or(HostError::Journal)?;
    connection
        .execute(statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn upgrade_objects(connection: &turso::Connection) -> Result<(), HostError> {
    rebuild_if_v18(
        connection,
        RebuildSpec {
            table: "capacity_reservation_provisioning_objects",
            legacy: "capacity_reservation_provisioning_objects_v18",
            expected_v18_columns: &["reservation_id", "role", "docker_name", "docker_id", "create_state"],
            expected_v19_columns: &["reservation_id", "role", "docker_name", "docker_id", "publication_policy_digest", "create_state"],
            v18_signature: SchemaSignature::SqlFragment("'dind-data'))"),
            v19_signature: SchemaSignature::SqlFragment("publication_policy_digest"),
            triggers: &["provisioning_object_no_delete", "provisioning_object_transition"],
            indexes: &[],
            copy_sql: "INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) SELECT reservation_id, role, docker_name, docker_id, NULL, create_state FROM capacity_reservation_provisioning_objects_v18",
        },
    )
    .await
}

async fn upgrade_v18_attachments(connection: &turso::Connection) -> Result<(), HostError> {
    rebuild_if_v18(
        connection,
        RebuildSpec {
            table: "capacity_reservation_bound_storage_attachments",
            legacy: "capacity_reservation_bound_storage_attachments_v18",
            expected_v18_columns: &ATTACHMENT_COLUMNS,
            expected_v19_columns: &ATTACHMENT_COLUMNS,
            v18_signature: SchemaSignature::SqlFragment(
                "observer = 'probe' and container_id is null and role = 'engine-metadata'",
            ),
            v19_signature: SchemaSignature::SqlFragment(
                "mount_target in ('/.velnor-runtime-identity', '/opt/velnor/job-facts')",
            ),
            triggers: &["bound_attachment_no_update", "bound_attachment_no_delete"],
            indexes: &["capacity_bound_attachment_mount"],
            copy_sql: "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments_v18",
        },
    )
    .await
}

async fn upgrade_publications(connection: &turso::Connection) -> Result<(), HostError> {
    rebuild_if_v18(
        connection,
        RebuildSpec {
            table: "capacity_reservation_provisioning_publications",
            legacy: "capacity_reservation_provisioning_publications_v18",
            expected_v18_columns: &PUBLICATION_V18_COLUMNS,
            expected_v19_columns: &PUBLICATION_V19_COLUMNS,
            v18_signature: SchemaSignature::ExactTable {
                ddl: VERSION_EIGHTEEN_PUBLICATION_DDL,
                primary_key: &["reservation_id"],
            },
            v19_signature: SchemaSignature::PrimaryKey(&["reservation_id", "target"]),
            triggers: &["provisioning_publication_no_update", "provisioning_publication_no_delete"],
            indexes: &[],
            copy_sql: "INSERT INTO capacity_reservation_provisioning_publications (reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only) SELECT reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only FROM capacity_reservation_provisioning_publications_v18",
        },
    )
    .await
}

async fn upgrade_cleanups(connection: &turso::Connection) -> Result<(), HostError> {
    rebuild_if_v18(
        connection,
        RebuildSpec {
            table: "capacity_reservation_provisioning_cleanups",
            legacy: "capacity_reservation_provisioning_cleanups_v18",
            expected_v18_columns: &CLEANUP_V18_COLUMNS,
            expected_v19_columns: &CLEANUP_V19_COLUMNS,
            v18_signature: SchemaSignature::SqlFragment("dind_data_volume"),
            v19_signature: SchemaSignature::SqlFragment("metadata_volume"),
            triggers: &["provisioning_cleanup_no_update", "provisioning_cleanup_no_delete"],
            indexes: &[],
            copy_sql: "INSERT INTO capacity_reservation_provisioning_cleanups (reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, metadata_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent) SELECT reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, NULL, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent FROM capacity_reservation_provisioning_cleanups_v18",
        },
    )
    .await
}

async fn mark_existing_topology_legacy(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) SELECT p.reservation_id, 1 FROM capacity_reservation_provisioning AS p WHERE NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS t WHERE t.reservation_id = p.reservation_id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

struct RebuildSpec<'a> {
    table: &'a str,
    legacy: &'a str,
    expected_v18_columns: &'a [&'a str],
    expected_v19_columns: &'a [&'a str],
    v18_signature: SchemaSignature<'a>,
    v19_signature: SchemaSignature<'a>,
    triggers: &'a [&'a str],
    indexes: &'a [&'a str],
    copy_sql: &'a str,
}

async fn rebuild_if_v18(
    connection: &turso::Connection,
    spec: RebuildSpec<'_>,
) -> Result<(), HostError> {
    let columns = table_columns(connection, spec.table).await?;
    let sql = table_sql(connection, spec.table).await?;
    if columns
        .iter()
        .map(String::as_str)
        .eq(spec.expected_v19_columns.iter().copied())
        && matches_signature(connection, spec.table, &sql, spec.v19_signature).await?
    {
        return Ok(());
    }
    if !columns
        .iter()
        .map(String::as_str)
        .eq(spec.expected_v18_columns.iter().copied())
        || !matches_signature(connection, spec.table, &sql, spec.v18_signature).await?
    {
        return Err(HostError::Journal);
    }
    for trigger in spec.triggers {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for index in spec.indexes {
        connection
            .execute(&format!("DROP INDEX IF EXISTS {index}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            &format!("ALTER TABLE {} RENAME TO {}", spec.table, spec.legacy),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let create_sql = TABLES
        .iter()
        .find(|(name, _)| *name == spec.table)
        .map(|(_, statement)| *statement)
        .ok_or(HostError::Journal)?;
    connection
        .execute(create_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(spec.copy_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {}", spec.legacy), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn contains_sql_fragment(sql: &str, fragment: &str) -> bool {
    let normalized_sql = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    let normalized_fragment = fragment
        .to_ascii_lowercase()
        .replace(char::is_whitespace, "");
    normalized_sql.contains(&normalized_fragment)
}

#[derive(Clone, Copy)]
enum SchemaSignature<'a> {
    SqlFragment(&'a str),
    PrimaryKey(&'a [&'a str]),
    ExactTable {
        ddl: &'a str,
        primary_key: &'a [&'a str],
    },
}

async fn matches_signature(
    connection: &turso::Connection,
    table: &str,
    sql: &str,
    signature: SchemaSignature<'_>,
) -> Result<bool, HostError> {
    match signature {
        SchemaSignature::SqlFragment(fragment) => Ok(contains_sql_fragment(sql, fragment)),
        SchemaSignature::PrimaryKey(expected) => Ok(primary_key_columns(connection, table)
            .await?
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied())),
        SchemaSignature::ExactTable { ddl, primary_key } => {
            let actual_key = primary_key_columns(connection, table).await?;
            Ok(canonical_table_sql(sql) == canonical_table_sql(ddl)
                && actual_key
                    .iter()
                    .map(String::as_str)
                    .eq(primary_key.iter().copied()))
        }
    }
}

fn canonical_table_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    if normalized.starts_with("createtableifnotexists") {
        normalized.replace_range(.."createtableifnotexists".len(), "createtable");
    }
    normalized
}

async fn primary_key_columns(
    connection: &turso::Connection,
    table: &str,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(&format!("PRAGMA table_info({table})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let ordinal = row.get::<i64>(5).map_err(|_| HostError::Journal)?;
        if ordinal > 0 {
            let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
            columns.push((ordinal, name));
        }
    }
    columns.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    Ok(columns.into_iter().map(|(_, name)| name).collect())
}

async fn table_sql(connection: &turso::Connection, table: &str) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

const ATTACHMENT_COLUMNS: [&str; 17] = [
    "reservation_id",
    "role",
    "observer",
    "container_id",
    "mount_target",
    "source_target",
    "read_only",
    "mount_id",
    "device_major",
    "device_minor",
    "statx_device_major",
    "statx_device_minor",
    "fs_type_magic",
    "v16_filesystem_id_0",
    "v16_filesystem_id_1",
    "filesystem_id",
    "filesystem_instance_digest",
];

const PUBLICATION_V18_COLUMNS: [&str; 15] = [
    "reservation_id",
    "runner_id",
    "runner_name",
    "runner_scale_set_id",
    "target",
    "projection_sha256",
    "directory_device_major",
    "directory_device_minor",
    "directory_inode",
    "mount_id",
    "v16_filesystem_id_0",
    "v16_filesystem_id_1",
    "filesystem_id",
    "filesystem_instance_digest",
    "read_only",
];

const PUBLICATION_V19_COLUMNS: [&str; 15] = PUBLICATION_V18_COLUMNS;

const CLEANUP_V18_COLUMNS: [&str; 13] = [
    "reservation_id",
    "runner_container_id",
    "dind_container_id",
    "shared_run_volume",
    "runner_home_volume",
    "runner_work_volume",
    "dind_data_volume",
    "instance_binding_digest",
    "role_mask",
    "containers_absent",
    "volumes_absent",
    "mounts_detached",
    "cgroups_absent",
];

const CLEANUP_V19_COLUMNS: [&str; 14] = [
    "reservation_id",
    "runner_container_id",
    "dind_container_id",
    "shared_run_volume",
    "runner_home_volume",
    "runner_work_volume",
    "dind_data_volume",
    "metadata_volume",
    "instance_binding_digest",
    "role_mask",
    "containers_absent",
    "volumes_absent",
    "mounts_detached",
    "cgroups_absent",
];
