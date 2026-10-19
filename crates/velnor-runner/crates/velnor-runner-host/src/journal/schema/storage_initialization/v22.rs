const V22_COLUMNS: [(&str, &str); 4] = [
    (
        "layout_version",
        "INTEGER NOT NULL DEFAULT 1 CHECK (layout_version IN (1, 2))",
    ),
    ("metadata_volume", "TEXT"),
    ("metadata_volume_id", "TEXT"),
    ("metadata_layout_digest", "TEXT"),
];

const V22_TRIGGERS: [(&str, &str); 3] = [
    (
        "storage_initialization_v2_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_v2_insert_guard BEFORE INSERT ON capacity_reservation_storage_initialization WHEN (NEW.layout_version = 1 AND (NEW.metadata_volume IS NOT NULL OR NEW.metadata_volume_id IS NOT NULL OR NEW.metadata_layout_digest IS NOT NULL)) OR (NEW.layout_version = 2 AND (NEW.metadata_volume IS NULL OR NEW.metadata_volume_id IS NULL OR NEW.metadata_volume != NEW.metadata_volume_id OR length(NEW.metadata_volume) != 42 OR substr(NEW.metadata_volume, 1, 1) != 'w' OR substr(NEW.metadata_volume, 34) != '-metadata' OR substr(NEW.metadata_volume, 2, 32) GLOB '*[^0-9a-f]*' OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND role = 'engine-metadata' AND create_state = 'created' AND docker_id = NEW.metadata_volume_id AND docker_name = NEW.metadata_volume) != 1)) BEGIN SELECT RAISE(ABORT, 'The storage initializer record is incorrect.'); END",
    ),
    (
        "storage_initialization_v2_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_v2_identity_guard BEFORE UPDATE OF layout_version, metadata_volume, metadata_volume_id ON capacity_reservation_storage_initialization BEGIN SELECT RAISE(ABORT, 'Do not change the storage initializer identity.'); END",
    ),
    (
        "storage_initialization_v2_result_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_v2_result_guard BEFORE UPDATE OF metadata_layout_digest, result_state ON capacity_reservation_storage_initialization WHEN (NEW.layout_version = 1 AND NEW.metadata_layout_digest IS NOT NULL) OR (NEW.layout_version = 2 AND ((NEW.result_state = 'confirmed' AND (NEW.metadata_layout_digest IS NULL OR length(NEW.metadata_layout_digest) != 64 OR NEW.metadata_layout_digest GLOB '*[^0-9a-f]*')) OR (NEW.result_state != 'confirmed' AND NEW.metadata_layout_digest IS NOT NULL))) OR (OLD.metadata_layout_digest IS NOT NULL AND OLD.metadata_layout_digest IS NOT NEW.metadata_layout_digest) BEGIN SELECT RAISE(ABORT, 'The storage initializer result is incorrect.'); END",
    ),
];

pub(super) async fn upgrade_v22(connection: &turso::Connection) -> Result<(), HostError> {
    let columns =
        super::read_table_columns(connection, "capacity_reservation_storage_initialization")
            .await?;
    for (name, definition) in V22_COLUMNS {
        if !columns.contains_key(name) {
            connection
                .execute(
                    &format!("ALTER TABLE capacity_reservation_storage_initialization ADD COLUMN {name} {definition}"),
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    for (_, statement) in V22_TRIGGERS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn repair_v27_metadata_volume_name_guard(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(
            "DROP TRIGGER IF EXISTS storage_initialization_v2_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(V22_TRIGGERS[0].1, ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

pub(super) async fn validate_v22(connection: &turso::Connection) -> Result<(), HostError> {
    let columns =
        super::read_table_columns(connection, "capacity_reservation_storage_initialization")
            .await?;
    for (name, expected) in V22_COLUMNS {
        let Some(actual) = columns.get(name) else {
            return Err(HostError::Journal);
        };
        let declared = expected
            .split_whitespace()
            .next()
            .ok_or(HostError::Journal)?;
        if !actual.declared_type.eq_ignore_ascii_case(declared)
            || (name == "layout_version"
                && (!actual.not_null || actual.default_value.as_deref() != Some("1")))
            || (name != "layout_version" && (actual.not_null || actual.default_value.is_some()))
        {
            return Err(HostError::Journal);
        }
    }
    for (name, expected) in V22_TRIGGERS {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [name],
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
    for query in V22_VALIDATION {
        if scalar_count(connection, query).await? != 0 {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

const V22_VALIDATION: &[&str] = &[
    "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE layout_version NOT IN (1, 2)",
    "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE (layout_version = 1 AND (metadata_volume IS NOT NULL OR metadata_volume_id IS NOT NULL OR metadata_layout_digest IS NOT NULL)) OR (layout_version = 2 AND (metadata_volume IS NULL OR metadata_volume_id IS NULL OR metadata_volume != metadata_volume_id))",
    "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE (layout_version = 2 AND result_state = 'confirmed' AND (metadata_layout_digest IS NULL OR length(metadata_layout_digest) != 64 OR metadata_layout_digest GLOB '*[^0-9a-f]*')) OR (result_state != 'confirmed' AND metadata_layout_digest IS NOT NULL)",
    "SELECT COUNT(*) FROM capacity_reservation_storage_initialization AS init WHERE init.layout_version = 2 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = init.reservation_id AND object.role = 'engine-metadata' AND object.create_state = 'created' AND object.docker_id = init.metadata_volume_id AND object.docker_name = init.metadata_volume) != 1",
];

#[cfg(test)]
mod tests {
    use super::{TABLE, V22_COLUMNS, table_definition_matches_with_v22};

    #[test]
    fn v22_fallback_preserves_quoted_check_literals() {
        let mut migrated = TABLE
            .strip_suffix(')')
            .expect("table DDL ends with a close parenthesis")
            .to_owned();
        for (name, definition) in V22_COLUMNS {
            migrated.push_str(", ");
            migrated.push_str(name);
            migrated.push(' ');
            migrated.push_str(definition);
        }
        migrated.push(')');
        assert!(
            table_definition_matches_with_v22(&migrated, TABLE),
            "actual={:?} expected={:?}",
            super::canonical_table_parts(&migrated),
            super::canonical_table_parts(TABLE)
        );

        let changed = migrated.replace("'uncertain'", "'un certain'");
        assert!(!table_definition_matches_with_v22(&changed, TABLE));
    }
}
