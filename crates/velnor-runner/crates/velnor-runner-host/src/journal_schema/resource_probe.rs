//! Additive schema for controller-owned resource-probe operations.

use turso::Connection;

use crate::error::HostError;

pub(super) async fn bootstrap(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS resource_probe_operations (operation_id TEXT PRIMARY KEY CHECK (length(operation_id) = 32 AND operation_id NOT GLOB '*[^0-9a-f]*'), instance_id TEXT NOT NULL, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), source_revision TEXT NOT NULL CHECK (length(source_revision) = 40 AND source_revision NOT GLOB '*[^0-9a-f]*'), runtime_image_id TEXT NOT NULL, image_binding_digest TEXT NOT NULL CHECK (length(image_binding_digest) = 64 AND image_binding_digest NOT GLOB '*[^0-9a-f]*'), operation_name TEXT NOT NULL UNIQUE, role TEXT NOT NULL CHECK (role = 'resource-probe'), projection_digest TEXT NOT NULL CHECK (length(projection_digest) = 64 AND projection_digest NOT GLOB '*[^0-9a-f]*'), phase TEXT NOT NULL CHECK (phase IN ('prepared', 'create_requested', 'container_created', 'start_requested', 'started', 'wait_requested', 'waited', 'logs_requested', 'output_observed', 'stop_requested', 'remove_requested', 'removed', 'aborted', 'quarantined')), container_id TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS resource_probe_one_active ON resource_probe_operations (role) WHERE phase NOT IN ('removed', 'aborted')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
