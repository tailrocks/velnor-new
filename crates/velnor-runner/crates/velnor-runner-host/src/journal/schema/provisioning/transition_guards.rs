const LEGACY_CONTAINER_TRANSITION: &str = "CREATE TRIGGER IF NOT EXISTS provisioning_container_transition BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.role != NEW.role OR OLD.reservation_id != NEW.reservation_id OR OLD.container_name != NEW.container_name OR NOT ((OLD.create_state = NEW.create_state AND OLD.container_id IS NEW.container_id AND OLD.start_state = NEW.start_state) OR (OLD.create_state = 'unclaimed' AND NEW.create_state = 'requested' AND NEW.container_id IS NULL AND NEW.start_state = 'not_requested') OR (OLD.create_state = 'requested' AND NEW.create_state = 'created' AND NEW.container_id IS NOT NULL AND NEW.start_state = 'not_requested') OR (OLD.create_state = 'requested' AND NEW.create_state = 'uncertain' AND NEW.container_id IS NULL AND NEW.start_state = 'not_requested') OR (OLD.create_state = 'created' AND OLD.container_id = NEW.container_id AND ((OLD.start_state = 'not_requested' AND NEW.start_state = 'requested') OR (OLD.start_state = 'requested' AND NEW.start_state IN ('started', 'uncertain'))))) BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END";
const LEGACY_STATE_TRANSITION: &str = "CREATE TRIGGER IF NOT EXISTS provisioning_state_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN OLD.reservation_id != NEW.reservation_id OR NOT ((OLD.bootstrap_start_state = NEW.bootstrap_start_state AND OLD.runner_container_id IS NEW.runner_container_id AND OLD.dind_container_id IS NEW.dind_container_id) OR (OLD.bootstrap_start_state = 'not_requested' AND NEW.bootstrap_start_state = 'requested' AND NEW.runner_container_id IS NULL AND NEW.dind_container_id IS NULL) OR (OLD.bootstrap_start_state = 'requested' AND NEW.bootstrap_start_state = 'started' AND NEW.runner_container_id IS NOT NULL AND NEW.dind_container_id IS NOT NULL) OR (OLD.bootstrap_start_state = 'requested' AND NEW.bootstrap_start_state = 'uncertain' AND NEW.runner_container_id IS NULL AND NEW.dind_container_id IS NULL)) OR NOT ((OLD.resources_bound = NEW.resources_bound AND OLD.instance_binding_digest IS NEW.instance_binding_digest AND OLD.guest_boot_id IS NEW.guest_boot_id) OR (OLD.resources_bound = 0 AND NEW.resources_bound = 1 AND NEW.instance_binding_digest IS NOT NULL AND NEW.guest_boot_id IS NOT NULL)) OR NOT ((OLD.jit_state = NEW.jit_state AND OLD.runner_id IS NEW.runner_id AND OLD.runner_name IS NEW.runner_name AND OLD.runner_scale_set_id IS NEW.runner_scale_set_id) OR (OLD.jit_state = 'not_requested' AND NEW.jit_state = 'requested' AND NEW.runner_id IS NULL AND NEW.runner_name IS NULL AND NEW.runner_scale_set_id IS NULL) OR (OLD.jit_state = 'requested' AND NEW.jit_state = 'bound' AND NEW.runner_id > 0 AND NEW.runner_name IS NOT NULL AND NEW.runner_scale_set_id > 0) OR (OLD.jit_state = 'requested' AND NEW.jit_state = 'uncertain' AND NEW.runner_id IS NULL AND NEW.runner_name IS NULL AND NEW.runner_scale_set_id IS NULL) OR (OLD.jit_state = 'bound' AND NEW.jit_state = 'cleaned' AND NEW.runner_id = OLD.runner_id AND NEW.runner_name = OLD.runner_name AND NEW.runner_scale_set_id = OLD.runner_scale_set_id)) OR NOT ((OLD.publication_state = NEW.publication_state AND OLD.projection_sha256 IS NEW.projection_sha256 AND OLD.publication_directory_device IS NEW.publication_directory_device AND OLD.publication_directory_inode IS NEW.publication_directory_inode AND OLD.publication_mount_id IS NEW.publication_mount_id) OR (OLD.publication_state = 'not_requested' AND NEW.publication_state = 'requested' AND NEW.projection_sha256 IS NULL AND NEW.publication_directory_device IS NULL AND NEW.publication_directory_inode IS NULL AND NEW.publication_mount_id IS NULL) OR (OLD.publication_state = 'requested' AND NEW.publication_state = 'published' AND NEW.projection_sha256 IS NOT NULL AND NEW.publication_directory_device IS NOT NULL AND NEW.publication_directory_inode IS NOT NULL AND NEW.publication_mount_id > 0) OR (OLD.publication_state = 'requested' AND NEW.publication_state = 'uncertain' AND NEW.projection_sha256 IS NULL AND NEW.publication_directory_device IS NULL AND NEW.publication_directory_inode IS NULL AND NEW.publication_mount_id IS NULL)) OR NOT ((OLD.delivery_state = NEW.delivery_state) OR (OLD.delivery_state = 'not_requested' AND NEW.delivery_state = 'requested' AND NEW.publication_state = 'published' AND NEW.jit_state = 'bound') OR (OLD.delivery_state = 'requested' AND NEW.delivery_state IN ('delivered', 'uncertain'))) OR NOT ((OLD.storage_cleanup_proven = NEW.storage_cleanup_proven) OR (OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NEW.resources_bound = 1)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END";

const CONTAINER_TRANSITION_TABLE: &str = "capacity_reservation_bootstrap_containers";
const STATE_TRANSITION_TABLE: &str = "capacity_reservation_provisioning_state";

fn is_transition_guard(name: &str) -> bool {
    name.starts_with("provisioning_container_") && name.ends_with("_guard")
        || name.starts_with("provisioning_container_") && name.ends_with("_transition")
        || name.starts_with("provisioning_state_") && name.ends_with("_guard")
        || name.starts_with("provisioning_state_") && name.ends_with("_transition")
        || name == "provisioning_container_transition"
        || name == "provisioning_state_transition"
}

async fn transition_table_exists(
    connection: &turso::Connection,
    table: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            (table,),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        == 1)
}

async fn reconcile_transition_guards(
    connection: &turso::Connection,
    allow_fresh_tables: bool,
) -> Result<(), HostError> {
    let old_container = transition_table_guard_set_matches(
        connection,
        CONTAINER_TRANSITION_TABLE,
        "provisioning_container_no_delete",
        Some(("provisioning_container_transition", LEGACY_CONTAINER_TRANSITION)),
    )
    .await?;
    let new_container = current_transition_table_guard_set_matches(
        connection,
        CONTAINER_TRANSITION_TABLE,
        "provisioning_container_no_delete",
        "provisioning_container_transition",
    )
    .await?;
    let old_state = transition_table_guard_set_matches(
        connection,
        STATE_TRANSITION_TABLE,
        "provisioning_state_no_delete",
        Some(("provisioning_state_transition", LEGACY_STATE_TRANSITION)),
    )
    .await?;
    let new_state = current_transition_table_guard_set_matches(
        connection,
        STATE_TRANSITION_TABLE,
        "provisioning_state_no_delete",
        "provisioning_state_transition",
    )
    .await?;
    let fresh = allow_fresh_tables
        && transition_table_guard_set_is_empty(connection, CONTAINER_TRANSITION_TABLE).await?
        && transition_table_guard_set_is_empty(connection, STATE_TRANSITION_TABLE).await?;

    if old_container && old_state {
        connection
            .execute("DROP TRIGGER provisioning_container_transition", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute("DROP TRIGGER provisioning_state_transition", ())
            .await
            .map_err(|_| HostError::Journal)?;
    } else if !(new_container && new_state) && !fresh {
        return Err(HostError::Journal);
    }

    for (name, statement) in TRIGGERS.iter().filter(|(name, _)| {
        is_transition_guard(name)
            || *name == "provisioning_container_no_delete"
            || *name == "provisioning_state_no_delete"
    }) {
        let _ = name;
        connection
            .execute(*statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }

    if !current_transition_table_guard_set_matches(
        connection,
        CONTAINER_TRANSITION_TABLE,
        "provisioning_container_no_delete",
        "provisioning_container_transition",
    )
    .await?
        || !current_transition_table_guard_set_matches(
            connection,
            STATE_TRANSITION_TABLE,
            "provisioning_state_no_delete",
            "provisioning_state_transition",
        )
        .await?
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn transition_table_guard_set_matches(
    connection: &turso::Connection,
    table: &str,
    no_delete_name: &str,
    transition: Option<(&str, &str)>,
) -> Result<bool, HostError> {
    let Some((transition_name, transition_sql)) = transition else {
        return Ok(false);
    };
    let Some((_, no_delete_sql)) = TRIGGERS.iter().find(|(name, _)| *name == no_delete_name) else {
        return Err(HostError::Journal);
    };
    let rows = read_table_triggers(connection, table).await?;
    if rows.len() != 2 {
        return Ok(false);
    }
    Ok(rows.iter().any(|(name, sql)| name == no_delete_name && normalize(sql) == normalize(no_delete_sql))
        && rows.iter().any(|(name, sql)| name == transition_name && normalize(sql) == normalize(transition_sql)))
}

async fn current_transition_table_guard_set_matches(
    connection: &turso::Connection,
    table: &str,
    no_delete_name: &str,
    legacy_transition_name: &str,
) -> Result<bool, HostError> {
    let rows = read_table_triggers(connection, table).await?;
    let mut expected = TRIGGERS
        .iter()
        .filter(|(name, _)| is_transition_guard(name) && trigger_targets_table(name, table))
        .collect::<Vec<_>>();
    if let Some(no_delete) = TRIGGERS.iter().find(|(name, _)| *name == no_delete_name) {
        expected.push(no_delete);
    }
    if rows.len() != expected.len() {
        return Ok(false);
    }
    if rows.iter().any(|(name, _)| name == legacy_transition_name) {
        return Ok(false);
    }
    Ok(expected.iter().all(|(name, sql)| {
        rows.iter()
            .any(|(actual_name, actual_sql)| actual_name == name && normalize(actual_sql) == normalize(sql))
    }))
}

fn trigger_targets_table(name: &str, table: &str) -> bool {
    match table {
        CONTAINER_TRANSITION_TABLE => name.starts_with("provisioning_container_"),
        STATE_TRANSITION_TABLE => name.starts_with("provisioning_state_"),
        _ => false,
    }
}

async fn transition_table_guard_set_is_empty(
    connection: &turso::Connection,
    table: &str,
) -> Result<bool, HostError> {
    Ok(read_table_triggers(connection, table).await?.is_empty())
}

async fn read_table_triggers(
    connection: &turso::Connection,
    table: &str,
) -> Result<Vec<(String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ?1 AND ((?1 = 'capacity_reservation_bootstrap_containers' AND name GLOB 'provisioning_container_*') OR (?1 = 'capacity_reservation_provisioning_state' AND name GLOB 'provisioning_state_*')) ORDER BY name",
            (table,),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut triggers = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        triggers.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
        ));
    }
    Ok(triggers)
}

pub(super) async fn upgrade_transition_guards(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    reconcile_transition_guards(connection, false).await
}
