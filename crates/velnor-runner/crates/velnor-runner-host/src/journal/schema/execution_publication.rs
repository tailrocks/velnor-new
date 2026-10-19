//! Durable parent identity for one execution-publication helper.

use crate::error::HostError;

const CONTEXT_TABLE: &str = "CREATE TABLE IF NOT EXISTS execution_publication_operations (operation_id TEXT PRIMARY KEY REFERENCES resource_probe_operations(operation_id), parent_kind TEXT NOT NULL CHECK (parent_kind IN ('challenge', 'decision', 'job-facts')), parent_operation_id INTEGER NOT NULL CHECK (parent_operation_id > 0), reservation_id INTEGER NOT NULL REFERENCES capacity_reservations(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), witness_request_id INTEGER CHECK (witness_request_id IS NULL OR witness_request_id > 0), actual_request_id INTEGER CHECK (actual_request_id IS NULL OR actual_request_id > 0), runner_id INTEGER NOT NULL CHECK (runner_id > 0), runner_name TEXT NOT NULL CHECK (length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128), runner_container_id TEXT NOT NULL CHECK (length(CAST(runner_container_id AS BLOB)) BETWEEN 12 AND 128), dind_container_id TEXT NOT NULL CHECK (length(CAST(dind_container_id AS BLOB)) BETWEEN 12 AND 128), engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), metadata_volume_name TEXT NOT NULL CHECK (length(CAST(metadata_volume_name AS BLOB)) BETWEEN 1 AND 255), metadata_volume_id TEXT NOT NULL CHECK (length(CAST(metadata_volume_id AS BLOB)) BETWEEN 1 AND 255), metadata_layout_version INTEGER NOT NULL CHECK (metadata_layout_version = 2), metadata_layout_digest TEXT NOT NULL CHECK (length(metadata_layout_digest) = 64 AND metadata_layout_digest NOT GLOB '*[^0-9a-f]*'), guest_boot_id TEXT NOT NULL CHECK (length(guest_boot_id) = 36), payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'), claim_digest TEXT NOT NULL CHECK (length(claim_digest) = 64 AND claim_digest NOT GLOB '*[^0-9a-f]*'), projection_digest TEXT NOT NULL CHECK (length(projection_digest) = 64 AND projection_digest NOT GLOB '*[^0-9a-f]*'), context_digest TEXT NOT NULL CHECK (length(context_digest) = 64 AND context_digest NOT GLOB '*[^0-9a-f]*'), recovery_projection_json TEXT CHECK (recovery_projection_json IS NULL OR length(CAST(recovery_projection_json AS BLOB)) <= 32768), recovery_projection_sha256 TEXT CHECK (recovery_projection_sha256 IS NULL OR (length(recovery_projection_sha256) = 64 AND recovery_projection_sha256 NOT GLOB '*[^0-9a-f]*')), CHECK ((recovery_projection_json IS NULL AND recovery_projection_sha256 IS NULL) OR (recovery_projection_json IS NOT NULL AND recovery_projection_sha256 IS NOT NULL)), CHECK (runner_container_id != dind_container_id), CHECK ((parent_kind = 'challenge' AND witness_request_id > 0 AND actual_request_id IS NULL) OR (parent_kind = 'decision' AND witness_request_id > 0 AND actual_request_id IS NOT NULL) OR (parent_kind = 'job-facts' AND witness_request_id IS NULL AND actual_request_id IS NULL)))";

const CONTEXT_INSERT_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_publication_context_insert_guard BEFORE INSERT ON execution_publication_operations WHEN NOT EXISTS (
SELECT 1 FROM resource_probe_operations AS operation
JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = operation.operation_id
JOIN capacity_reservation_storage_initialization AS metadata ON metadata.reservation_id = NEW.reservation_id
JOIN capacity_reservation_provisioning_state AS worker_state ON worker_state.reservation_id = NEW.reservation_id
LEFT JOIN connection_execution_proofs AS proof ON proof.intent_id = NEW.parent_operation_id
WHERE operation.operation_id = NEW.operation_id
AND operation.helper_role = 'execution-publication' AND operation.role = 'resource-probe'
AND operation.probe_purpose = 'none' AND operation.request_sha256 IS NULL
AND operation.phase = 'prepared' AND operation.container_id IS NULL
AND operation.operation_name = 'velnor-execution-publication-' || operation.operation_id
AND operation.projection_digest = NEW.projection_digest
AND lease.state = 'reserved' AND lease.container_id IS NULL
AND lease.operation_name = operation.operation_name
AND lease.engine_id = operation.engine_id AND lease.docker_root_digest = operation.docker_root_digest
AND metadata.layout_version = 2 AND metadata.result_state = 'confirmed'
AND metadata.metadata_volume = NEW.metadata_volume_name
AND metadata.metadata_volume_id = NEW.metadata_volume_id
AND metadata.metadata_layout_digest = NEW.metadata_layout_digest
AND worker_state.resources_bound = 1
AND worker_state.guest_boot_id = NEW.guest_boot_id
AND NEW.recovery_projection_json IS NOT NULL AND NEW.recovery_projection_sha256 IS NOT NULL
AND ((NEW.parent_kind = 'challenge'
AND proof.reservation_id = NEW.reservation_id AND proof.connection_id = NEW.connection_id
AND proof.scale_set_id = NEW.scale_set_id AND proof.witness_request_id = NEW.witness_request_id
AND proof.runner_id = NEW.runner_id AND proof.runner_name = NEW.runner_name
AND proof.runner_container_id = NEW.runner_container_id AND proof.dind_container_id = NEW.dind_container_id
AND proof.engine_id = NEW.engine_id AND proof.docker_root_digest = NEW.docker_root_digest
AND NEW.actual_request_id IS NULL AND proof.challenge_state = 'requested'
AND proof.challenge_digest = NEW.claim_digest AND proof.challenge_digest = NEW.payload_sha256)
OR (NEW.parent_kind = 'decision'
AND proof.reservation_id = NEW.reservation_id AND proof.connection_id = NEW.connection_id
AND proof.scale_set_id = NEW.scale_set_id AND proof.witness_request_id = NEW.witness_request_id
AND proof.runner_id = NEW.runner_id AND proof.runner_name = NEW.runner_name
AND proof.runner_container_id = NEW.runner_container_id AND proof.dind_container_id = NEW.dind_container_id
AND proof.engine_id = NEW.engine_id AND proof.docker_root_digest = NEW.docker_root_digest
AND proof.actual_request_id = NEW.actual_request_id AND proof.proof_state IN ('approved', 'denied')
AND EXISTS (SELECT 1 FROM connection_execution_decisions AS decision
WHERE decision.intent_id = NEW.parent_operation_id AND decision.reservation_id = NEW.reservation_id
AND decision.connection_id = NEW.connection_id AND decision.scale_set_id = NEW.scale_set_id
AND decision.witness_request_id = NEW.witness_request_id
AND decision.actual_request_id = NEW.actual_request_id
AND decision.decision_digest = NEW.claim_digest AND decision.decision_digest = NEW.payload_sha256
AND decision.delivery_state = 'requested'))
OR (NEW.parent_kind = 'job-facts' AND NEW.witness_request_id IS NULL
AND NEW.actual_request_id IS NULL
AND EXISTS (SELECT 1 FROM capacity_reservations AS reservation
JOIN connection_intents AS intent ON intent.id = reservation.intent_id
JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id
JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id
WHERE reservation.id = NEW.reservation_id AND reservation.connection_id = NEW.connection_id
AND reservation.state = 'reserved' AND intent.connection_id = NEW.connection_id
AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.launch_phase = 'jit_bound'
AND state.reservation_id = NEW.reservation_id AND state.resources_bound = 1
AND state.bootstrap_start_state = 'started' AND state.jit_state = 'bound'
AND state.publication_state = 'requested' AND state.delivery_state = 'not_requested'
AND state.storage_cleanup_proven = 0 AND state.runner_id = NEW.runner_id
AND state.runner_name = NEW.runner_name AND state.runner_scale_set_id = NEW.scale_set_id
AND state.runner_container_id = NEW.runner_container_id AND state.dind_container_id = NEW.dind_container_id
AND state.guest_boot_id = NEW.guest_boot_id AND jit.state = 'bound'
AND jit.runner_id = NEW.runner_id AND jit.runner_name = NEW.runner_name
AND jit.scale_set_id = NEW.scale_set_id))))
BEGIN SELECT RAISE(ABORT, 'invalid execution publication context'); END";

const CONTEXT_UPDATE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_publication_context_update_guard BEFORE UPDATE ON execution_publication_operations BEGIN SELECT RAISE(ABORT, 'execution publication context is immutable'); END";
const CONTEXT_DELETE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_publication_context_delete_guard BEFORE DELETE ON execution_publication_operations BEGIN SELECT RAISE(ABORT, 'execution publication context is retained'); END";

const PARENT_CLEANUP_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_publication_parent_cleanup_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND EXISTS (SELECT 1 FROM execution_publication_operations AS publication JOIN resource_probe_operations AS operation ON operation.operation_id = publication.operation_id WHERE publication.reservation_id = NEW.reservation_id AND operation.phase NOT IN ('removed', 'aborted')) BEGIN SELECT RAISE(ABORT, 'execution publication helper is not removed'); END";

const PARENT_RELEASE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_publication_parent_release_guard BEFORE UPDATE OF state ON capacity_reservations WHEN NEW.state = 'released' AND EXISTS (SELECT 1 FROM execution_publication_operations AS publication JOIN resource_probe_operations AS operation ON operation.operation_id = publication.operation_id WHERE publication.reservation_id = NEW.id AND operation.phase NOT IN ('removed', 'aborted')) BEGIN SELECT RAISE(ABORT, 'execution publication helper is not removed'); END";

const HELPER_ROLE_COLUMN: &str = "helper_role TEXT NOT NULL DEFAULT 'resource-probe' CHECK (helper_role IN ('resource-probe', 'execution-publication'))";
const PROBE_PURPOSE_COLUMN: &str = "probe_purpose TEXT NOT NULL DEFAULT 'capacity' CHECK (probe_purpose IN ('capacity', 'owned-cgroups', 'none'))";
const REQUEST_SHA256_COLUMN: &str = "request_sha256 TEXT CHECK (request_sha256 IS NULL OR (length(request_sha256) = 64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'))";
const REQUEST_CONTEXT_COLUMN: &str = "request_context TEXT CHECK (request_context IS NULL OR length(CAST(request_context AS BLOB)) <= 8192)";

/// Add closed helper identity columns and the publication parent context.
pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    ensure_column(connection, "helper_role", HELPER_ROLE_COLUMN).await?;
    ensure_column(connection, "probe_purpose", PROBE_PURPOSE_COLUMN).await?;
    ensure_column(connection, "request_sha256", REQUEST_SHA256_COLUMN).await?;
    ensure_column(connection, "request_context", REQUEST_CONTEXT_COLUMN).await?;
    let capacity_request_sha256 = crate::journal::capacity_request_sha256();
    connection
        .execute(
            "UPDATE resource_probe_operations SET request_sha256 = ?1 WHERE helper_role = 'resource-probe' AND probe_purpose = 'capacity' AND request_context IS NULL AND (request_sha256 IS NULL OR request_sha256 = '5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf')",
            (capacity_request_sha256.as_str(),),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(CONTEXT_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    ensure_context_column(
        connection,
        "recovery_projection_json",
        "recovery_projection_json TEXT CHECK (recovery_projection_json IS NULL OR length(CAST(recovery_projection_json AS BLOB)) <= 32768)",
    )
    .await?;
    ensure_context_column(
        connection,
        "recovery_projection_sha256",
        "recovery_projection_sha256 TEXT CHECK (recovery_projection_sha256 IS NULL OR (length(recovery_projection_sha256) = 64 AND recovery_projection_sha256 NOT GLOB '*[^0-9a-f]*'))",
    )
    .await?;
    for trigger in [
        CONTEXT_INSERT_GUARD,
        CONTEXT_UPDATE_GUARD,
        CONTEXT_DELETE_GUARD,
        PARENT_CLEANUP_GUARD,
        PARENT_RELEASE_GUARD,
    ] {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

/// Reject incomplete or mismatched helper-operation records.
pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    validate_columns(connection).await?;
    validate_context_columns(connection).await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM resource_probe_operations AS operation LEFT JOIN execution_publication_operations AS publication ON publication.operation_id = operation.operation_id WHERE (operation.helper_role = 'resource-probe' AND (operation.probe_purpose NOT IN ('capacity', 'owned-cgroups') OR operation.request_sha256 IS NULL OR publication.operation_id IS NOT NULL)) OR (operation.helper_role = 'execution-publication' AND (operation.probe_purpose != 'none' OR operation.request_sha256 IS NOT NULL OR publication.operation_id IS NULL))",
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
    if count != 0 {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM execution_publication_operations AS publication JOIN resource_probe_operations AS operation ON operation.operation_id = publication.operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = operation.operation_id WHERE operation.helper_role != 'execution-publication' OR operation.role != 'resource-probe' OR publication.engine_id != operation.engine_id OR publication.docker_root_digest != operation.docker_root_digest OR operation.operation_name != 'velnor-execution-publication-' || operation.operation_id OR publication.projection_digest != operation.projection_digest OR lease.operation_name != operation.operation_name OR lease.engine_id != operation.engine_id OR lease.docker_root_digest != operation.docker_root_digest",
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
    if count != 0 {
        return Err(HostError::Journal);
    }
    validate_trigger(
        connection,
        "execution_publication_context_insert_guard",
        CONTEXT_INSERT_GUARD,
    )
    .await?;
    validate_trigger(
        connection,
        "execution_publication_context_update_guard",
        CONTEXT_UPDATE_GUARD,
    )
    .await?;
    validate_trigger(
        connection,
        "execution_publication_context_delete_guard",
        CONTEXT_DELETE_GUARD,
    )
    .await?;
    validate_trigger(
        connection,
        "execution_publication_parent_cleanup_guard",
        PARENT_CLEANUP_GUARD,
    )
    .await?;
    validate_trigger(
        connection,
        "execution_publication_parent_release_guard",
        PARENT_RELEASE_GUARD,
    )
    .await?;
    Ok(())
}

async fn validate_trigger(
    connection: &turso::Connection,
    name: &str,
    expected: &str,
) -> Result<(), HostError> {
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
    if normalize(&actual) == normalize(expected) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn normalize(sql: &str) -> String {
    let mut normalized = String::with_capacity(sql.len());
    let mut quote = None;
    let mut chars = sql.chars().peekable();
    while let Some(character) = chars.next() {
        if let Some(delimiter) = quote {
            normalized.push(character);
            let closing = if delimiter == '[' { ']' } else { delimiter };
            if character == closing {
                if delimiter != '[' && chars.peek() == Some(&closing) {
                    normalized.push(chars.next().unwrap_or(closing));
                } else {
                    quote = None;
                }
            }
        } else if matches!(character, '\'' | '"' | '`' | '[') {
            normalized.push(character);
            quote = Some(character);
        } else if character.is_whitespace() {
            let next = chars.clone().find(|next| !next.is_whitespace());
            let previous_is_punctuation = normalized.chars().last().is_some_and(is_sql_punctuation);
            let next_is_punctuation = next.is_some_and(is_sql_punctuation);
            if !previous_is_punctuation && !next_is_punctuation {
                normalized.push(' ');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalize_left_outer_join(&normalized)
}

/// Treat the SQL parser's equivalent outer-join spelling as the same token sequence.
fn normalize_left_outer_join(sql: &str) -> String {
    const CANONICAL: &str = "left outer join";
    let mut output = String::with_capacity(sql.len());
    let mut index = 0;
    let mut quote = None;
    let bytes = sql.as_bytes();
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            let closing = if delimiter == b'[' { b']' } else { delimiter };
            if bytes[index] == closing {
                output.push(char::from(bytes[index]));
                index += 1;
                if delimiter != b'[' && bytes.get(index) == Some(&closing) {
                    output.push(char::from(bytes[index]));
                    index += 1;
                    continue;
                }
                quote = None;
            } else {
                let character = sql[index..].chars().next().unwrap_or_default();
                output.push(character);
                index += character.len_utf8();
            }
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`' | b'[') {
            quote = Some(bytes[index]);
            output.push(char::from(bytes[index]));
            index += 1;
            continue;
        }
        let end = index + CANONICAL.len();
        let before_is_word = index > 0 && is_sql_word(bytes[index - 1]);
        let after_is_word = bytes.get(end).is_some_and(|byte| is_sql_word(*byte));
        if !before_is_word && !after_is_word && bytes.get(index..end) == Some(CANONICAL.as_bytes())
        {
            output.push_str("left join");
            index = end;
        } else {
            let character = sql[index..].chars().next().unwrap_or_default();
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}

fn is_sql_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_sql_punctuation(character: char) -> bool {
    matches!(character, '(' | ')' | ',' | ';')
}

mod migration;
use migration::{ensure_column, ensure_context_column, validate_columns, validate_context_columns};
pub(in crate::journal::schema) use migration::{upgrade_v25, upgrade_v27, validate_v27};

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn trigger_normalization_accepts_sqlite_punctuation_spacing() {
        assert_eq!(
            normalize("SELECT RAISE(ABORT, 'kept text')"),
            normalize("SELECT RAISE ( ABORT , 'kept text' )")
        );
        assert_ne!(
            normalize("SELECT RAISE(ABORT, 'kept text')"),
            normalize("SELECT RAISE(ABORT, 'changed text')")
        );
        assert_ne!(
            normalize("SELECT RAISE(ABORT, 'requested')"),
            normalize("SELECT RAISE(ABORT, 'REQUESTED')")
        );
        assert_ne!(
            normalize("SELECT RAISE(ABORT, 'two words')"),
            normalize("SELECT RAISE(ABORT, 'twowords')")
        );
        assert_eq!(
            normalize("LEFT JOIN proof ON proof.id = row.id"),
            normalize("LEFT OUTER JOIN proof ON proof.id = row.id")
        );
        assert_ne!(
            normalize("LEFT JOIN proof ON proof.id = row.id"),
            normalize("LEFT OUTER JOIN proof ON proof.id = row.other_id")
        );
        assert_ne!(
            normalize("SELECT 'LEFT OUTER JOIN'"),
            normalize("SELECT 'LEFT JOIN'")
        );
        assert_ne!(
            normalize("SELECT \"LEFT OUTER JOIN\""),
            normalize("SELECT \"LEFT JOIN\"")
        );
        assert_ne!(
            normalize("SELECT [LEFT OUTER JOIN]"),
            normalize("SELECT [LEFT JOIN]")
        );
        assert_ne!(
            normalize("SELECT `LEFT OUTER JOIN`"),
            normalize("SELECT `LEFT JOIN`")
        );
        assert_ne!(
            normalize("SELECT \"Requested\""),
            normalize("SELECT \"REQUESTED\"")
        );
        assert_ne!(
            normalize("SELECT \"a ( b )\""),
            normalize("SELECT \"a(b)\"")
        );
        assert_ne!(
            normalize("SELECT [two words]"),
            normalize("SELECT [two  words]")
        );
        assert_ne!(
            normalize("SELECT 'a'' LEFT OUTER JOIN ẞ'"),
            normalize("SELECT 'a'' LEFT JOIN ẞ'")
        );
        assert_ne!(
            normalize("SELECT xleft outer join row"),
            normalize("SELECT left join row")
        );
    }
}
