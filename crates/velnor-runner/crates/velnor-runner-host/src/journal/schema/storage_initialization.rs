//! Durable storage initialization state.

use super::sql_normalization::normalize;
use crate::error::HostError;

const TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_storage_initialization (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservations(id), connection_id TEXT NOT NULL, operation_id INTEGER NOT NULL, generation INTEGER NOT NULL CHECK (generation = 1), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), image_runtime_id TEXT NOT NULL CHECK (length(image_runtime_id) = 71 AND image_runtime_id GLOB 'sha256:*' AND substr(image_runtime_id, 8) NOT GLOB '*[^0-9a-f]*'), image_source_sha TEXT NOT NULL CHECK (length(image_source_sha) = 40 AND image_source_sha NOT GLOB '*[^0-9a-f]*'), nano_cpus INTEGER NOT NULL CHECK (nano_cpus > 0), memory_bytes INTEGER NOT NULL CHECK (memory_bytes > 0), pids_limit INTEGER NOT NULL CHECK (pids_limit BETWEEN 1 AND 16), home_volume TEXT NOT NULL CHECK (length(home_volume) BETWEEN 1 AND 64), work_volume TEXT NOT NULL CHECK (length(work_volume) BETWEEN 1 AND 64 AND work_volume != home_volume), helper_name TEXT NOT NULL CHECK (length(helper_name) BETWEEN 1 AND 128), helper_id TEXT CHECK (helper_id IS NULL OR (length(helper_id) = 64 AND helper_id NOT GLOB '*[^0-9a-f]*')), abort_helper_id TEXT CHECK (abort_helper_id IS NULL OR (length(abort_helper_id) = 64 AND abort_helper_id NOT GLOB '*[^0-9a-f]*')), abort_claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (abort_claim_generation >= 0), abort_lease_until INTEGER NOT NULL DEFAULT 0 CHECK (abort_lease_until >= 0), create_dispatch_state TEXT NOT NULL DEFAULT 'not_dispatched' CHECK (create_dispatch_state IN ('not_dispatched', 'dispatched', 'response_received', 'outcome_unknown')), create_state TEXT NOT NULL CHECK (create_state IN ('requested', 'response_recorded', 'created', 'uncertain')), start_state TEXT NOT NULL CHECK (start_state IN ('not_requested', 'requested', 'started', 'uncertain')), result_state TEXT NOT NULL CHECK (result_state IN ('not_requested', 'confirmed', 'uncertain')), remove_state TEXT NOT NULL CHECK (remove_state IN ('not_requested', 'requested', 'removed', 'uncertain')), abort_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (abort_state IN ('not_requested', 'requested', 'remove_requested', 'absent', 'uncertain')), operation_key TEXT NOT NULL CHECK (length(operation_key) BETWEEN 1 AND 64), home_device_major INTEGER, home_device_minor INTEGER, home_inode INTEGER, home_mount_id INTEGER, work_device_major INTEGER, work_device_minor INTEGER, work_inode INTEGER, work_mount_id INTEGER, CHECK ((create_state IN ('response_recorded', 'created') AND helper_id IS NOT NULL) OR (create_state = 'requested' AND helper_id IS NULL) OR create_state = 'uncertain'), CHECK ((result_state = 'confirmed' AND home_device_major IS NOT NULL AND home_device_major BETWEEN 0 AND 4294967295) OR (result_state != 'confirmed' AND home_device_major IS NULL)), CHECK ((result_state = 'confirmed' AND home_device_minor IS NOT NULL AND home_device_minor BETWEEN 0 AND 4294967295) OR (result_state != 'confirmed' AND home_device_minor IS NULL)), CHECK ((result_state = 'confirmed' AND home_inode IS NOT NULL AND home_inode >= 1) OR (result_state != 'confirmed' AND home_inode IS NULL)), CHECK ((result_state = 'confirmed' AND home_mount_id IS NOT NULL AND home_mount_id >= 1) OR (result_state != 'confirmed' AND home_mount_id IS NULL)), CHECK ((result_state = 'confirmed' AND work_device_major IS NOT NULL AND work_device_major BETWEEN 0 AND 4294967295) OR (result_state != 'confirmed' AND work_device_major IS NULL)), CHECK ((result_state = 'confirmed' AND work_device_minor IS NOT NULL AND work_device_minor BETWEEN 0 AND 4294967295) OR (result_state != 'confirmed' AND work_device_minor IS NULL)), CHECK ((result_state = 'confirmed' AND work_inode IS NOT NULL AND work_inode >= 1) OR (result_state != 'confirmed' AND work_inode IS NULL)), CHECK ((result_state = 'confirmed' AND work_mount_id IS NOT NULL AND work_mount_id >= 1) OR (result_state != 'confirmed' AND work_mount_id IS NULL)), CHECK (result_state != 'confirmed' OR home_mount_id != work_mount_id), CHECK (result_state != 'confirmed' OR home_device_major != work_device_major OR home_device_minor != work_device_minor OR home_inode != work_inode))";
const TRIGGERS: [(&str, &str); 18] = [
    (
        "storage_initialization_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_insert_guard BEFORE INSERT ON capacity_reservation_storage_initialization WHEN NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id WHERE reservation.id = NEW.reservation_id AND reservation.intent_id = NEW.operation_id AND reservation.connection_id = NEW.connection_id AND reservation.engine_id = NEW.engine_id AND reservation.docker_root_digest = NEW.docker_root_digest AND intent.connection_id = NEW.connection_id AND intent.kind = 'launch') OR NEW.operation_key != CAST(NEW.operation_id AS TEXT) || '-1' OR NEW.helper_name != 'velnor-init-' || NEW.operation_key OR NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = NEW.reservation_id AND state.bootstrap_start_state = 'not_requested' AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS container WHERE container.reservation_id = state.reservation_id AND (container.create_state != 'unclaimed' OR container.start_state != 'not_requested'))) OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND role = 'runner-home' AND create_state = 'created' AND docker_id = docker_name AND docker_name = NEW.home_volume) != 1 OR (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND role = 'runner-work' AND create_state = 'created' AND docker_id = docker_name AND docker_name = NEW.work_volume) != 1 BEGIN SELECT RAISE(ABORT, 'The storage initializer record is incorrect.'); END",
    ),
    (
        "storage_initialization_identity_immutable",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_identity_immutable BEFORE UPDATE OF reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, helper_name, operation_key ON capacity_reservation_storage_initialization BEGIN SELECT RAISE(ABORT, 'Do not change the storage initializer identity.'); END",
    ),
    (
        "storage_initialization_no_delete",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_no_delete BEFORE DELETE ON capacity_reservation_storage_initialization BEGIN SELECT RAISE(ABORT, 'Do not remove the storage initializer record.'); END",
    ),
    (
        "storage_initialization_dispatch_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_dispatch_transition_guard BEFORE UPDATE OF create_dispatch_state ON capacity_reservation_storage_initialization WHEN NOT ((OLD.create_dispatch_state = 'not_dispatched' AND NEW.create_dispatch_state IN ('dispatched', 'response_received')) OR (OLD.create_dispatch_state = 'dispatched' AND NEW.create_dispatch_state IN ('not_dispatched', 'response_received', 'outcome_unknown')) OR (OLD.create_dispatch_state = 'outcome_unknown' AND NEW.create_dispatch_state = 'response_received') OR OLD.create_dispatch_state = NEW.create_dispatch_state) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_create_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_create_transition_guard BEFORE UPDATE OF create_state, helper_id ON capacity_reservation_storage_initialization WHEN NOT ((OLD.create_state = 'requested' AND NEW.create_state IN ('response_recorded', 'uncertain')) OR (OLD.create_state = 'response_recorded' AND NEW.create_state IN ('created', 'uncertain')) OR (OLD.create_state = 'created' AND NEW.create_state = 'uncertain') OR OLD.create_state = NEW.create_state) OR (OLD.create_state = 'uncertain' AND NEW.create_state != 'uncertain') OR (OLD.helper_id IS NOT NULL AND OLD.helper_id IS NOT NEW.helper_id) OR (OLD.helper_id IS NULL AND NEW.helper_id IS NOT NULL AND NOT (OLD.create_state = 'requested' AND NEW.create_state = 'response_recorded')) OR (NEW.start_state != 'not_requested' AND NEW.create_state != 'created') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_start_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_start_transition_guard BEFORE UPDATE OF start_state ON capacity_reservation_storage_initialization WHEN NOT ((OLD.start_state = 'not_requested' AND NEW.start_state = 'requested') OR (OLD.start_state = 'requested' AND NEW.start_state IN ('started', 'uncertain')) OR OLD.start_state = NEW.start_state) OR (OLD.start_state = 'uncertain' AND NEW.start_state != 'uncertain') OR (NEW.start_state != 'not_requested' AND NEW.create_state != 'created') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_result_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_result_transition_guard BEFORE UPDATE OF result_state, home_device_major, home_device_minor, home_inode, home_mount_id, work_device_major, work_device_minor, work_inode, work_mount_id ON capacity_reservation_storage_initialization WHEN NOT ((OLD.result_state = 'not_requested' AND NEW.result_state IN ('confirmed', 'uncertain')) OR OLD.result_state = NEW.result_state) OR (OLD.result_state = 'uncertain' AND NEW.result_state != 'uncertain') OR (NEW.result_state = 'confirmed' AND NEW.start_state != 'started') OR (OLD.result_state = 'confirmed' AND (OLD.home_device_major IS NOT NEW.home_device_major OR OLD.home_device_minor IS NOT NEW.home_device_minor OR OLD.home_inode IS NOT NEW.home_inode OR OLD.home_mount_id IS NOT NEW.home_mount_id OR OLD.work_device_major IS NOT NEW.work_device_major OR OLD.work_device_minor IS NOT NEW.work_device_minor OR OLD.work_inode IS NOT NEW.work_inode OR OLD.work_mount_id IS NOT NEW.work_mount_id)) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_new_state_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_new_state_guard BEFORE UPDATE OF abort_state ON capacity_reservation_storage_initialization WHEN OLD.abort_state = 'not_requested' AND NEW.abort_state NOT IN ('not_requested', 'requested', 'uncertain') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_requested_state_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_requested_state_guard BEFORE UPDATE OF abort_state ON capacity_reservation_storage_initialization WHEN OLD.abort_state = 'requested' AND NEW.abort_state NOT IN ('requested', 'remove_requested', 'absent', 'uncertain') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_remove_state_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_remove_state_guard BEFORE UPDATE OF abort_state ON capacity_reservation_storage_initialization WHEN OLD.abort_state = 'remove_requested' AND NEW.abort_state NOT IN ('remove_requested', 'requested', 'absent', 'uncertain') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_uncertain_state_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_uncertain_state_guard BEFORE UPDATE OF abort_state ON capacity_reservation_storage_initialization WHEN OLD.abort_state = 'uncertain' AND NEW.abort_state NOT IN ('uncertain', 'requested') BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_id_immutable_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_id_immutable_guard BEFORE UPDATE OF abort_helper_id ON capacity_reservation_storage_initialization WHEN OLD.abort_helper_id IS NOT NULL AND OLD.abort_helper_id IS NOT NEW.abort_helper_id BEGIN SELECT RAISE(ABORT, 'Do not change the storage initializer identity.'); END",
    ),
    (
        "storage_initialization_abort_id_state_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_id_state_guard BEFORE UPDATE OF abort_helper_id ON capacity_reservation_storage_initialization WHEN OLD.abort_helper_id IS NULL AND NEW.abort_helper_id IS NOT NULL AND NEW.abort_state != 'remove_requested' BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_id_format_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_id_format_guard BEFORE UPDATE OF abort_helper_id, abort_state, helper_id ON capacity_reservation_storage_initialization WHEN NEW.abort_state = 'remove_requested' AND (NEW.abort_helper_id IS NULL OR length(NEW.abort_helper_id) != 64 OR NEW.abort_helper_id GLOB '*[^0-9a-f]*' OR (NEW.helper_id IS NOT NULL AND NEW.abort_helper_id != NEW.helper_id)) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_generation_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_generation_guard BEFORE UPDATE OF abort_claim_generation ON capacity_reservation_storage_initialization WHEN NEW.abort_claim_generation < OLD.abort_claim_generation OR NEW.abort_claim_generation > OLD.abort_claim_generation + 1 BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_lease_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_lease_guard BEFORE UPDATE OF abort_lease_until ON capacity_reservation_storage_initialization WHEN NEW.abort_lease_until != OLD.abort_lease_until AND NOT ((NEW.abort_lease_until > OLD.abort_lease_until AND NEW.abort_lease_until > 0 AND NEW.abort_claim_generation > 0) OR (NEW.abort_lease_until = 0 AND NEW.abort_state IN ('absent', 'uncertain'))) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_abort_phase_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_abort_phase_guard BEFORE UPDATE OF abort_state ON capacity_reservation_storage_initialization WHEN (NEW.abort_state = 'requested' AND (NEW.abort_claim_generation = 0 OR NEW.abort_lease_until = 0)) OR (NEW.abort_state != 'not_requested' AND (NEW.create_state NOT IN ('requested', 'response_recorded', 'created', 'uncertain') OR NEW.start_state NOT IN ('not_requested', 'requested', 'started', 'uncertain') OR NEW.result_state NOT IN ('not_requested', 'confirmed', 'uncertain') OR NEW.remove_state NOT IN ('not_requested', 'requested', 'removed', 'uncertain'))) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
    (
        "storage_initialization_remove_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS storage_initialization_remove_transition_guard BEFORE UPDATE OF remove_state ON capacity_reservation_storage_initialization WHEN NOT ((OLD.remove_state = 'not_requested' AND NEW.remove_state = 'requested') OR (OLD.remove_state = 'requested' AND NEW.remove_state IN ('removed', 'uncertain')) OR OLD.remove_state = NEW.remove_state) OR (OLD.remove_state = 'uncertain' AND NEW.remove_state != 'uncertain') OR (NEW.remove_state != 'not_requested' AND NEW.result_state != 'confirmed') OR (NEW.remove_state = 'removed' AND NEW.helper_id IS NULL) BEGIN SELECT RAISE(ABORT, 'The storage initializer state change is incorrect.'); END",
    ),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
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
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in
        std::iter::once(("capacity_reservation_storage_initialization", TABLE)).chain(TRIGGERS)
    {
        let kind = if name.starts_with("storage_initialization_") {
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
        let actual = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?;
        let matches = if name == "capacity_reservation_storage_initialization" {
            table_definition_matches_with_v22(&actual, expected)
        } else {
            normalize(&actual) == normalize(expected)
        };
        if !matches {
            return Err(HostError::Journal);
        }
    }
    validate_parent_reference(connection).await?;
    for query in VALIDATION_QUERIES {
        if scalar_count(connection, query).await? != 0 {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_parent_reference(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "PRAGMA foreign_key_list(capacity_reservation_storage_initialization)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if row.get::<String>(2).map_err(|_| HostError::Journal)? != "capacity_reservations"
        || row.get::<String>(3).map_err(|_| HostError::Journal)? != "reservation_id"
        || row.get::<String>(4).map_err(|_| HostError::Journal)? != "id"
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

include!("storage_initialization/validation.rs");

include!("storage_initialization/v22.rs");

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

fn table_definition_matches_with_v22(actual: &str, expected: &str) -> bool {
    let (Some(mut actual), Some(mut expected)) = (
        canonical_table_parts(actual),
        canonical_table_parts(expected),
    ) else {
        return false;
    };
    let mut base = actual.clone();
    base.sort();
    expected.sort();
    if base == expected {
        return true;
    }
    for (name, definition) in V22_COLUMNS {
        let addition = normalize(&format!("{name} {definition}"));
        let Some(index) = actual.iter().position(|part| *part == addition) else {
            return false;
        };
        actual.remove(index);
    }
    actual.sort();
    actual == expected
}

fn canonical_table_parts(sql: &str) -> Option<Vec<String>> {
    let bytes = sql.as_bytes();
    let start = bytes.iter().position(|byte| *byte == b'(')?;
    let mut parts = Vec::new();
    let mut depth = 1_u32;
    let mut in_string = false;
    let mut part_start = start + 1;
    let mut index = part_start;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' if in_string && bytes.get(index + 1) == Some(&b'\'') => index += 1,
            b'\'' => in_string = !in_string,
            b'(' if !in_string => depth += 1,
            b')' if !in_string => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    parts.push(sql.get(part_start..index)?.to_owned());
                    if !sql.get(index + 1..)?.trim().is_empty() {
                        return None;
                    }
                    return Some(canonicalize_table_parts(parts));
                }
            }
            b',' if !in_string && depth == 1 => {
                parts.push(sql.get(part_start..index)?.to_owned());
                part_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn canonicalize_table_parts(parts: Vec<String>) -> Vec<String> {
    let parent_reference = normalize("REFERENCES capacity_reservations(id)");
    let parent_constraint = normalize("FOREIGN KEY (reservation_id)");
    let table_parent_reference_sql =
        normalize("FOREIGN KEY (reservation_id) REFERENCES capacity_reservations(id)");
    let mut canonical = Vec::with_capacity(parts.len() + 1);
    let mut inline_parent_reference = false;
    let mut table_parent_reference = false;
    for part in parts {
        let normalized = normalize(part.trim());
        if normalized == table_parent_reference_sql {
            table_parent_reference = true;
            canonical.push(parent_constraint.clone());
        } else if normalized.contains(&parent_reference) {
            inline_parent_reference = true;
            canonical.push(
                normalize(&normalized.replace(&parent_reference, ""))
                    .trim()
                    .to_owned(),
            );
        } else {
            canonical.push(normalized);
        }
    }
    if inline_parent_reference && !table_parent_reference {
        canonical.push(parent_constraint);
    }
    canonical
}
