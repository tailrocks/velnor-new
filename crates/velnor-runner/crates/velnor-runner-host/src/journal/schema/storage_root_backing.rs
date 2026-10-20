use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::sql_normalization::normalize;
mod migration_v27;
mod migration_v30;
mod migration_v31;
pub(super) use migration_v27::upgrade_v27;
pub(super) use migration_v30::upgrade_v30;
pub(super) use migration_v31::upgrade_v31;
#[cfg(test)]
#[path = "storage_root_backing_tests.rs"]
mod tests;

const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_root_backing_mappings (reservation_id INTEGER NOT NULL REFERENCES capacity_reservations(id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL, sample_generation_id TEXT NOT NULL, role TEXT NOT NULL CHECK (role IN ('runner-root', 'dind-root')), container_id TEXT NOT NULL, image_runtime_id TEXT NOT NULL CHECK (length(CAST(image_runtime_id AS BLOB)) BETWEEN 1 AND 255 AND instr(image_runtime_id, char(0)) = 0), image_source_sha TEXT NOT NULL CHECK (length(image_source_sha) = 40 AND image_source_sha NOT GLOB '*[^0-9a-f]*'), logical_mount_id INTEGER NOT NULL CHECK (logical_mount_id > 0), logical_filesystem_digest TEXT NOT NULL, backing_filesystem_digest TEXT NOT NULL, mapping_record TEXT NOT NULL, mapping_digest TEXT NOT NULL, PRIMARY KEY (reservation_id, role))";
const ATTACHMENT_PATHS: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_paths (reservation_id INTEGER NOT NULL, role TEXT NOT NULL, observer TEXT NOT NULL, mount_target TEXT NOT NULL, mount_root TEXT NOT NULL CHECK (length(mount_root) BETWEEN 1 AND 512), volume_subpath TEXT, PRIMARY KEY (reservation_id, role, observer, mount_target), FOREIGN KEY (reservation_id, role, observer, mount_target) REFERENCES capacity_reservation_bound_storage_attachments(reservation_id, role, observer, mount_target), CHECK ((role = 'engine-metadata' AND observer = 'runner' AND ((mount_target = '/.velnor-runtime-identity' AND volume_subpath IS 'runtime-identity') OR (mount_target = '/opt/velnor/job-facts' AND volume_subpath IS 'job-facts') OR (mount_target = '/opt/velnor/source-proof/control' AND volume_subpath IS 'source-proof/control') OR (mount_target = '/opt/velnor/source-proof/inbox' AND volume_subpath IS 'source-proof/inbox'))) OR (role = 'engine-metadata' AND observer = 'dind' AND ((mount_target = '/.velnor-runtime-identity' AND volume_subpath IS 'runtime-identity') OR (mount_target = '/opt/velnor/job-facts' AND volume_subpath IS 'job-facts'))) OR (role = 'runner-home' AND ((observer IN ('runner', 'dind') AND mount_target = '/home/runner' AND volume_subpath IS NULL) OR (observer = 'runner' AND mount_target = '/opt/velnor/source-proof/inbox' AND volume_subpath IS NOT NULL AND volume_subpath IS 'source-proof/inbox'))) OR (role = 'engine-metadata' AND observer = 'probe' AND volume_subpath IS NULL) OR (role != 'engine-metadata' AND role != 'runner-home' AND volume_subpath IS NULL)))";
const V29_ATTACHMENT_PATHS: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_paths (reservation_id INTEGER NOT NULL, role TEXT NOT NULL, observer TEXT NOT NULL, mount_target TEXT NOT NULL, mount_root TEXT NOT NULL CHECK (length(mount_root) BETWEEN 1 AND 512), volume_subpath TEXT, PRIMARY KEY (reservation_id, role, observer, mount_target), FOREIGN KEY (reservation_id, role, observer, mount_target) REFERENCES capacity_reservation_bound_storage_attachments(reservation_id, role, observer, mount_target), CHECK ((role = 'engine-metadata' AND observer = 'runner' AND ((mount_target = '/.velnor-runtime-identity' AND volume_subpath IS 'runtime-identity') OR (mount_target = '/opt/velnor/job-facts' AND volume_subpath IS 'job-facts') OR (mount_target = '/opt/velnor/source-proof/control' AND volume_subpath IS 'source-proof/control') OR (mount_target = '/opt/velnor/source-proof/inbox' AND volume_subpath IS 'source-proof/inbox'))) OR (role = 'runner-home' AND ((observer IN ('runner', 'dind') AND mount_target = '/home/runner' AND volume_subpath IS NULL) OR (observer = 'runner' AND mount_target = '/opt/velnor/source-proof/inbox' AND volume_subpath IS NOT NULL AND volume_subpath IS 'source-proof/inbox'))) OR (role = 'engine-metadata' AND observer = 'probe' AND volume_subpath IS NULL) OR (role != 'engine-metadata' AND role != 'runner-home' AND volume_subpath IS NULL)))";
const LEGACY_ATTACHMENT_PATHS: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_paths (reservation_id INTEGER NOT NULL, role TEXT NOT NULL, observer TEXT NOT NULL, mount_target TEXT NOT NULL, mount_root TEXT NOT NULL CHECK (length(mount_root) BETWEEN 1 AND 512), volume_subpath TEXT, PRIMARY KEY (reservation_id, role, observer, mount_target), FOREIGN KEY (reservation_id, role, observer, mount_target) REFERENCES capacity_reservation_bound_storage_attachments(reservation_id, role, observer, mount_target), CHECK ((role = 'engine-metadata' AND observer = 'runner' AND ((mount_target = '/.velnor-runtime-identity' AND volume_subpath = 'runtime-identity') OR (mount_target = '/opt/velnor/job-facts' AND volume_subpath = 'job-facts') OR (mount_target = '/opt/velnor/source-proof/control' AND volume_subpath = 'source-proof/control') OR (mount_target = '/opt/velnor/source-proof/inbox' AND volume_subpath = 'source-proof/inbox'))) OR (role = 'engine-metadata' AND observer = 'probe' AND volume_subpath IS NULL) OR (role != 'engine-metadata' AND volume_subpath IS NULL)))";

const TRIGGERS: [(&str, &str); 7] = [
    (
        "root_backing_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS root_backing_insert_guard BEFORE INSERT ON capacity_reservation_root_backing_mappings WHEN NOT EXISTS (SELECT 1 FROM capacity_reservations AS r JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN connection_intents AS i ON i.id = p.operation_id WHERE r.id = NEW.reservation_id AND r.connection_id = NEW.connection_id AND r.intent_id = NEW.operation_id AND p.operation_id = NEW.operation_id AND p.engine_id = NEW.engine_id AND p.docker_root_digest = NEW.docker_root_digest AND p.sample_generation_id = NEW.sample_generation_id AND s.bootstrap_start_state = 'started' AND s.resources_bound = 0 AND i.state = 'pending' AND i.cleanup_proven = 0) OR NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS c WHERE c.reservation_id = NEW.reservation_id AND c.role = CASE NEW.role WHEN 'runner-root' THEN 'runner' ELSE 'dind' END AND c.create_state = 'created' AND c.start_state = 'started' AND c.container_id = NEW.container_id) OR length(NEW.logical_filesystem_digest) != 64 OR NEW.logical_filesystem_digest GLOB '*[^0-9a-f]*' OR length(NEW.backing_filesystem_digest) != 64 OR NEW.backing_filesystem_digest GLOB '*[^0-9a-f]*' OR length(NEW.mapping_digest) != 64 OR NEW.mapping_digest GLOB '*[^0-9a-f]*' BEGIN SELECT RAISE(ABORT, 'The root backing record is incorrect.'); END",
    ),
    (
        "root_backing_identity_immutable",
        "CREATE TRIGGER IF NOT EXISTS root_backing_identity_immutable BEFORE UPDATE ON capacity_reservation_root_backing_mappings BEGIN SELECT RAISE(ABORT, 'Do not change the root backing record.'); END",
    ),
    (
        "root_backing_no_delete",
        "CREATE TRIGGER IF NOT EXISTS root_backing_no_delete BEFORE DELETE ON capacity_reservation_root_backing_mappings BEGIN SELECT RAISE(ABORT, 'Do not remove the root backing record.'); END",
    ),
    (
        "root_backing_bound_guard",
        "CREATE TRIGGER IF NOT EXISTS root_backing_bound_guard BEFORE UPDATE OF resources_bound ON capacity_reservation_provisioning_state WHEN OLD.resources_bound = 0 AND NEW.resources_bound = 1 AND ((SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings WHERE reservation_id = NEW.reservation_id AND role IN ('runner-root', 'dind-root')) != 2 OR (SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths WHERE reservation_id = NEW.reservation_id) != (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = NEW.reservation_id)) BEGIN SELECT RAISE(ABORT, 'The root backing record is incomplete.'); END",
    ),
    (
        "bound_storage_path_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS bound_storage_path_insert_guard BEFORE INSERT ON capacity_reservation_bound_storage_paths WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_attachments AS a JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = a.reservation_id WHERE a.reservation_id = NEW.reservation_id AND a.role = NEW.role AND a.observer = NEW.observer AND a.mount_target = NEW.mount_target AND s.resources_bound = 0) OR length(NEW.mount_root) NOT BETWEEN 1 AND 512 OR instr(NEW.mount_root, char(0)) > 0 BEGIN SELECT RAISE(ABORT, 'The observed storage path is incorrect.'); END",
    ),
    (
        "bound_storage_path_no_update",
        "CREATE TRIGGER IF NOT EXISTS bound_storage_path_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_paths BEGIN SELECT RAISE(ABORT, 'The observed storage path cannot change.'); END",
    ),
    (
        "bound_storage_path_no_delete",
        "CREATE TRIGGER IF NOT EXISTS bound_storage_path_no_delete BEFORE DELETE ON capacity_reservation_bound_storage_paths BEGIN SELECT RAISE(ABORT, 'Do not remove the observed storage path.'); END",
    ),
];

const PATH_INODE_TRIGGER: (&str, &str) = (
    "bound_storage_path_inode_insert_guard",
    "CREATE TRIGGER IF NOT EXISTS bound_storage_path_inode_insert_guard BEFORE INSERT ON capacity_reservation_bound_storage_paths WHEN NEW.statx_inode IS NULL OR NEW.statx_inode <= 0 BEGIN SELECT RAISE(ABORT, 'The storage path inode value must be more than zero.'); END",
);

pub(super) fn attachment_paths_with_inode() -> String {
    ATTACHMENT_PATHS.replace(
        "mount_root TEXT NOT NULL CHECK (length(mount_root) BETWEEN 1 AND 512), volume_subpath TEXT",
        "mount_root TEXT NOT NULL CHECK (length(mount_root) BETWEEN 1 AND 512), statx_inode INTEGER CHECK (statx_inode IS NULL OR statx_inode > 0), volume_subpath TEXT",
    )
}

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    let attachment_paths = attachment_paths_with_inode();
    connection
        .execute(&attachment_paths, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, sql) in TRIGGERS {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(PATH_INODE_TRIGGER.1, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn bootstrap_v30(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(ATTACHMENT_PATHS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, sql) in TRIGGERS {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let attachment_paths = attachment_paths_with_inode();
    validate_schema(connection, &attachment_paths, true).await
}

pub(super) async fn validate_v30(connection: &turso::Connection) -> Result<(), HostError> {
    validate_schema(connection, ATTACHMENT_PATHS, false).await
}

async fn validate_schema(
    connection: &turso::Connection,
    attachment_paths: &str,
    with_inode: bool,
) -> Result<(), HostError> {
    for (name, expected) in [
        ("capacity_reservation_root_backing_mappings", TABLE),
        ("capacity_reservation_bound_storage_paths", attachment_paths),
    ]
    .into_iter()
    .chain(TRIGGERS)
    .chain(with_inode.then_some(PATH_INODE_TRIGGER))
    {
        let kind = if expected.starts_with("CREATE TRIGGER") {
            "trigger"
        } else {
            "table"
        };
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let sql = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?;
        if normalize(&sql) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    for query in VALIDATION {
        let mut rows = connection
            .query(query, ())
            .await
            .map_err(|_| HostError::Journal)?;
        let count = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?;
        if count != 0 {
            return Err(HostError::Journal);
        }
    }
    let mut rows = connection
        .query(
            "SELECT mapping_record, mapping_digest FROM capacity_reservation_root_backing_mappings",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let record = row.get::<String>(0).map_err(|_| HostError::Journal)?;
        let digest = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        if digest_bytes(record.as_bytes()) != digest {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

const VALIDATION: &[&str] = &[
    "SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths AS x LEFT JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = x.reservation_id AND a.role = x.role AND a.observer = x.observer AND a.mount_target = x.mount_target WHERE a.reservation_id IS NULL OR length(x.mount_root) NOT BETWEEN 1 AND 512 OR (x.role = 'engine-metadata' AND x.observer = 'runner' AND NOT ((x.mount_target = '/.velnor-runtime-identity' AND x.volume_subpath IS 'runtime-identity') OR (x.mount_target = '/opt/velnor/job-facts' AND x.volume_subpath IS 'job-facts') OR (x.mount_target = '/opt/velnor/source-proof/control' AND x.volume_subpath IS 'source-proof/control') OR (x.mount_target = '/opt/velnor/source-proof/inbox' AND x.volume_subpath IS 'source-proof/inbox'))) OR (x.role = 'engine-metadata' AND x.observer = 'dind' AND NOT ((x.mount_target = '/.velnor-runtime-identity' AND x.volume_subpath IS 'runtime-identity') OR (x.mount_target = '/opt/velnor/job-facts' AND x.volume_subpath IS 'job-facts'))) OR (x.role = 'runner-home' AND NOT ((x.observer IN ('runner', 'dind') AND x.mount_target = '/home/runner' AND x.volume_subpath IS NULL) OR (x.observer = 'runner' AND x.mount_target = '/opt/velnor/source-proof/inbox' AND x.volume_subpath IS NOT NULL AND x.volume_subpath IS 'source-proof/inbox'))) OR ((x.role != 'engine-metadata' AND x.role != 'runner-home' OR x.observer = 'probe') AND x.volume_subpath IS NOT NULL)",
    "SELECT COUNT(*) FROM (SELECT x.reservation_id FROM capacity_reservation_bound_storage_paths AS x GROUP BY x.reservation_id HAVING COUNT(*) != (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = x.reservation_id))",
    "SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings WHERE role NOT IN ('runner-root', 'dind-root') OR logical_mount_id <= 0 OR length(CAST(image_runtime_id AS BLOB)) NOT BETWEEN 1 AND 255 OR length(image_source_sha) != 40 OR image_source_sha GLOB '*[^0-9a-f]*'",
    "SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings WHERE length(mapping_digest) != 64 OR mapping_digest GLOB '*[^0-9a-f]*' OR length(logical_filesystem_digest) != 64 OR logical_filesystem_digest GLOB '*[^0-9a-f]*' OR length(backing_filesystem_digest) != 64 OR backing_filesystem_digest GLOB '*[^0-9a-f]*'",
    "SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m LEFT JOIN capacity_reservations AS r ON r.id = m.reservation_id LEFT JOIN capacity_reservation_provisioning AS p ON p.reservation_id = m.reservation_id LEFT JOIN capacity_reservation_bootstrap_containers AS c ON c.reservation_id = m.reservation_id AND c.role = CASE m.role WHEN 'runner-root' THEN 'runner' ELSE 'dind' END WHERE r.id IS NULL OR r.connection_id != m.connection_id OR r.intent_id != m.operation_id OR p.engine_id != m.engine_id OR p.docker_root_digest != m.docker_root_digest OR p.sample_generation_id != m.sample_generation_id OR c.container_id != m.container_id",
    "SELECT COUNT(*) FROM (SELECT reservation_id FROM capacity_reservation_root_backing_mappings GROUP BY reservation_id HAVING COUNT(*) != 2)",
    "SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m LEFT JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = m.reservation_id AND a.role = m.role AND a.observer = CASE m.role WHEN 'runner-root' THEN 'runner' ELSE 'dind' END AND a.mount_target = '/' LEFT JOIN capacity_reservation_bound_storage_roles AS r ON r.reservation_id = m.reservation_id AND r.role = m.role WHERE a.reservation_id IS NULL OR a.container_id != m.container_id OR a.mount_id != m.logical_mount_id OR a.filesystem_instance_digest != m.logical_filesystem_digest OR r.filesystem_instance_digest != m.backing_filesystem_digest",
    "SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS s JOIN capacity_reservation_provisioning AS p ON p.reservation_id = s.reservation_id JOIN capacity_reservations AS r ON r.id = s.reservation_id WHERE s.resources_bound = 1 AND ((SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m WHERE m.reservation_id = s.reservation_id AND m.connection_id = r.connection_id AND m.operation_id = p.operation_id AND m.engine_id = p.engine_id AND m.docker_root_digest = p.docker_root_digest AND m.sample_generation_id = p.sample_generation_id) NOT IN (0, 2) OR (SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = s.reservation_id) NOT IN (0, (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = s.reservation_id)) OR ((SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m WHERE m.reservation_id = s.reservation_id) = 0 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = s.reservation_id) != 0))",
];

fn digest_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-root-backing-map-v1\0");
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
