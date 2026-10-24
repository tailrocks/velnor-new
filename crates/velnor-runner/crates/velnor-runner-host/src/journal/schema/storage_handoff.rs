use crate::error::HostError;

use super::sql_normalization::normalize;

#[cfg(test)]
#[path = "storage_handoff_tests.rs"]
mod tests;
const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_storage_handoffs (reservation_id INTEGER PRIMARY KEY REFERENCES capacity_reservation_storage_initialization(reservation_id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL, generation INTEGER NOT NULL CHECK (generation > 0), helper_name TEXT NOT NULL, helper_id TEXT NOT NULL CHECK (length(helper_id) = 64 AND helper_id NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL, metadata_volume_name TEXT NOT NULL, metadata_volume_id TEXT NOT NULL, operation_key TEXT NOT NULL, layout_digest TEXT NOT NULL CHECK (length(layout_digest) = 64 AND layout_digest NOT GLOB '*[^0-9a-f]*'), ready_digest TEXT NOT NULL CHECK (length(ready_digest) = 64 AND ready_digest NOT GLOB '*[^0-9a-f]*'), state TEXT NOT NULL CHECK (state IN ('ready_held', 'attached')), mount_evidence_digest TEXT CHECK (mount_evidence_digest IS NULL OR (length(mount_evidence_digest) = 64 AND mount_evidence_digest NOT GLOB '*[^0-9a-f]*')), runner_container_id TEXT CHECK (runner_container_id IS NULL OR (length(runner_container_id) = 64 AND runner_container_id NOT GLOB '*[^0-9a-f]*')), dind_container_id TEXT CHECK (dind_container_id IS NULL OR (length(dind_container_id) = 64 AND dind_container_id NOT GLOB '*[^0-9a-f]*')), CHECK ((state = 'ready_held' AND mount_evidence_digest IS NULL AND runner_container_id IS NULL AND dind_container_id IS NULL) OR (state = 'attached' AND mount_evidence_digest IS NOT NULL AND runner_container_id IS NOT NULL AND dind_container_id IS NOT NULL AND runner_container_id != dind_container_id)))";

const LEGACY_TRANSITION_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS storage_handoff_transition_guard BEFORE UPDATE ON capacity_reservation_storage_handoffs WHEN OLD.state != 'ready_held' OR NEW.state != 'attached' OR OLD.reservation_id != NEW.reservation_id OR OLD.connection_id != NEW.connection_id OR OLD.operation_id != NEW.operation_id OR OLD.generation != NEW.generation OR OLD.helper_name != NEW.helper_name OR OLD.helper_id != NEW.helper_id OR OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest OR OLD.metadata_volume_name != NEW.metadata_volume_name OR OLD.metadata_volume_id != NEW.metadata_volume_id OR OLD.operation_key != NEW.operation_key OR OLD.layout_digest != NEW.layout_digest OR OLD.ready_digest != NEW.ready_digest OR NEW.mount_evidence_digest IS NULL OR NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS runner WHERE runner.reservation_id = NEW.reservation_id AND runner.role = 'runner' AND runner.create_state = 'created' AND runner.start_state = 'started' AND runner.container_id = NEW.runner_container_id) OR NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS dind WHERE dind.reservation_id = NEW.reservation_id AND dind.role = 'dind' AND dind.create_state = 'created' AND dind.start_state = 'started' AND dind.container_id = NEW.dind_container_id) BEGIN SELECT RAISE(ABORT, 'Invalid storage handoff.'); END";

const TRANSITION_TRIGGERS: [(&str, &str); 3] = [
    (
        "storage_handoff_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_handoff_transition_guard BEFORE UPDATE ON capacity_reservation_storage_handoffs WHEN OLD.state != 'ready_held' OR NEW.state != 'attached' OR OLD.reservation_id != NEW.reservation_id OR OLD.connection_id != NEW.connection_id OR OLD.operation_id != NEW.operation_id OR OLD.generation != NEW.generation OR OLD.helper_name != NEW.helper_name OR OLD.helper_id != NEW.helper_id OR OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest OR OLD.metadata_volume_name != NEW.metadata_volume_name OR OLD.metadata_volume_id != NEW.metadata_volume_id OR OLD.operation_key != NEW.operation_key OR OLD.layout_digest != NEW.layout_digest OR OLD.ready_digest != NEW.ready_digest OR NEW.mount_evidence_digest IS NULL BEGIN SELECT RAISE(ABORT, 'Invalid storage handoff.'); END",
    ),
    (
        "storage_handoff_runner_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_handoff_runner_identity_guard BEFORE UPDATE ON capacity_reservation_storage_handoffs WHEN OLD.state = 'ready_held' AND NEW.state = 'attached' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS runner WHERE runner.reservation_id = NEW.reservation_id AND runner.role = 'runner' AND runner.create_state = 'created' AND runner.start_state = 'started' AND runner.container_id = NEW.runner_container_id) BEGIN SELECT RAISE(ABORT, 'Invalid storage handoff runner.'); END",
    ),
    (
        "storage_handoff_dind_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_handoff_dind_identity_guard BEFORE UPDATE ON capacity_reservation_storage_handoffs WHEN OLD.state = 'ready_held' AND NEW.state = 'attached' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS dind WHERE dind.reservation_id = NEW.reservation_id AND dind.role = 'dind' AND dind.create_state = 'created' AND dind.start_state = 'started' AND dind.container_id = NEW.dind_container_id) BEGIN SELECT RAISE(ABORT, 'Invalid storage handoff DinD.'); END",
    ),
];

const TRIGGERS: [(&str, &str); 3] = [
    (
        "storage_handoff_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_handoff_insert_guard BEFORE INSERT ON capacity_reservation_storage_handoffs WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS init JOIN capacity_reservations AS reservation ON reservation.id = init.reservation_id JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = init.reservation_id AND object.role = 'engine-metadata' AND object.create_state = 'created' WHERE init.reservation_id = NEW.reservation_id AND init.connection_id = NEW.connection_id AND init.operation_id = NEW.operation_id AND init.generation = NEW.generation AND init.create_state = 'created' AND init.start_state = 'started' AND init.result_state = 'confirmed' AND init.remove_state = 'not_requested' AND init.layout_version = 2 AND init.helper_name = NEW.helper_name AND init.helper_id = NEW.helper_id AND init.engine_id = NEW.engine_id AND init.docker_root_digest = NEW.docker_root_digest AND init.metadata_volume = NEW.metadata_volume_name AND init.metadata_volume_id = NEW.metadata_volume_id AND init.operation_key = NEW.operation_key AND init.metadata_layout_digest = NEW.layout_digest AND reservation.connection_id = NEW.connection_id AND reservation.intent_id = NEW.operation_id AND reservation.state = 'reserved' AND object.docker_name = NEW.metadata_volume_name AND object.docker_id = NEW.metadata_volume_id) OR NEW.state != 'ready_held' BEGIN SELECT RAISE(ABORT, 'Invalid storage handoff.'); END",
    ),
    (
        "storage_handoff_no_delete",
        "CREATE TRIGGER IF NOT EXISTS storage_handoff_no_delete BEFORE DELETE ON capacity_reservation_storage_handoffs BEGIN SELECT RAISE(ABORT, 'Keep the storage handoff record.'); END",
    ),
    (
        "storage_initializer_remove_requires_handoff",
        "CREATE TRIGGER IF NOT EXISTS storage_initializer_remove_requires_handoff BEFORE UPDATE OF remove_state ON capacity_reservation_storage_initialization WHEN NEW.remove_state = 'requested' AND OLD.result_state = 'confirmed' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage_handoffs AS handoff WHERE handoff.reservation_id = NEW.reservation_id AND handoff.connection_id = NEW.connection_id AND handoff.operation_id = NEW.operation_id AND handoff.state = 'attached') BEGIN SELECT RAISE(ABORT, 'Attach initialized storage before removing its helper.'); END",
    ),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    let table_existed =
        object_exists(connection, "table", "capacity_reservation_storage_handoffs").await?;
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, statement) in TRIGGERS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    transition_triggers(connection, table_existed).await?;
    Ok(())
}

async fn transition_triggers(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), HostError> {
    let legacy_exists =
        object_exists(connection, "trigger", "storage_handoff_transition_guard").await?;
    let mut extra_count = 0;
    for (name, _) in TRANSITION_TRIGGERS.iter().skip(1) {
        if object_exists(connection, "trigger", name).await? {
            extra_count += 1;
        }
    }

    if !table_existed {
        if legacy_exists || extra_count != 0 {
            return Err(HostError::Journal);
        }
        for (_, statement) in TRANSITION_TRIGGERS {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
        return Ok(());
    }

    if legacy_exists {
        let exact_legacy = trigger_matches(
            connection,
            "storage_handoff_transition_guard",
            LEGACY_TRANSITION_TRIGGER,
        )
        .await?;
        if exact_legacy {
            if extra_count != 0 {
                return Err(HostError::Journal);
            }
            connection
                .execute("DROP TRIGGER storage_handoff_transition_guard", ())
                .await
                .map_err(|_| HostError::Journal)?;
            for (_, statement) in TRANSITION_TRIGGERS {
                connection
                    .execute(statement, ())
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            return Ok(());
        }

        if extra_count != TRANSITION_TRIGGERS.len() - 1 {
            return Err(HostError::Journal);
        }
        for (name, expected) in TRANSITION_TRIGGERS {
            if !trigger_matches(connection, name, expected).await? {
                return Err(HostError::Journal);
            }
        }
        return Ok(());
    }

    Err(HostError::Journal)
}

async fn object_exists(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

async fn trigger_matches(
    connection: &turso::Connection,
    name: &str,
    expected: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
            [name],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let actual = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    Ok(normalize(&actual) == normalize(expected))
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_storage_handoffs'", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize(&actual) != normalize(TABLE) {
        return Err(HostError::Journal);
    }
    for (name, expected) in TRIGGERS.iter().chain(TRANSITION_TRIGGERS.iter()) {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [*name],
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
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_storage_handoffs AS handoff LEFT JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = handoff.reservation_id LEFT JOIN capacity_reservations AS reservation ON reservation.id = handoff.reservation_id WHERE init.reservation_id IS NULL OR reservation.connection_id != handoff.connection_id OR reservation.intent_id != handoff.operation_id OR init.connection_id != handoff.connection_id OR init.operation_id != handoff.operation_id OR init.generation != handoff.generation OR init.helper_id != handoff.helper_id OR init.engine_id != handoff.engine_id OR init.docker_root_digest != handoff.docker_root_digest OR init.metadata_volume != handoff.metadata_volume_name OR init.metadata_volume_id != handoff.metadata_volume_id OR init.operation_key != handoff.operation_key OR init.metadata_layout_digest != handoff.layout_digest OR length(handoff.ready_digest) != 64 OR handoff.ready_digest GLOB '*[^0-9a-f]*' OR init.result_state != 'confirmed' OR init.layout_version != 2 OR (handoff.state = 'ready_held' AND (init.remove_state != 'not_requested' OR handoff.mount_evidence_digest IS NOT NULL OR handoff.runner_container_id IS NOT NULL OR handoff.dind_container_id IS NOT NULL)) OR (handoff.state = 'attached' AND (init.remove_state NOT IN ('not_requested', 'requested', 'removed', 'uncertain') OR length(handoff.mount_evidence_digest) != 64 OR handoff.mount_evidence_digest GLOB '*[^0-9a-f]*' OR NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS runner WHERE runner.reservation_id = handoff.reservation_id AND runner.role = 'runner' AND runner.create_state = 'created' AND runner.start_state = 'started' AND runner.container_id = handoff.runner_container_id) OR NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS dind WHERE dind.reservation_id = handoff.reservation_id AND dind.role = 'dind' AND dind.create_state = 'created' AND dind.start_state = 'started' AND dind.container_id = handoff.dind_container_id)))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
