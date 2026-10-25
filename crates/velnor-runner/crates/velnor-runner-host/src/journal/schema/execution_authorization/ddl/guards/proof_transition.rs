use super::proof_insert::trigger_sql;

pub(in crate::journal::schema::execution_authorization::ddl) const LEGACY_PROOF_TRANSITION_GUARD:
    &str = r"CREATE TRIGGER IF NOT EXISTS execution_proof_transition_guard
BEFORE UPDATE ON connection_execution_proofs
WHEN OLD.intent_id != NEW.intent_id OR OLD.reservation_id != NEW.reservation_id
 OR OLD.connection_id != NEW.connection_id OR OLD.scale_set_id != NEW.scale_set_id
 OR OLD.witness_request_id != NEW.witness_request_id OR OLD.binding_digest != NEW.binding_digest
 OR OLD.generation != NEW.generation OR OLD.policy_digest != NEW.policy_digest
 OR OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest
 OR OLD.runner_id != NEW.runner_id OR OLD.runner_name != NEW.runner_name
 OR OLD.runner_container_id != NEW.runner_container_id OR OLD.dind_container_id != NEW.dind_container_id
 OR OLD.instance_binding_digest != NEW.instance_binding_digest OR OLD.challenge != NEW.challenge
 OR OLD.channel_digest != NEW.channel_digest OR OLD.channel_json != NEW.channel_json OR OLD.challenge_json != NEW.challenge_json
 OR OLD.challenge_digest != NEW.challenge_digest
 OR (OLD.source_digest IS NOT NULL AND OLD.source_digest != NEW.source_digest)
 OR (OLD.source_json IS NOT NULL AND OLD.source_json != NEW.source_json)
 OR NOT (
   (OLD.challenge_state = NEW.challenge_state)
   OR (OLD.challenge_state = 'not_requested' AND NEW.challenge_state = 'requested')
   OR (OLD.challenge_state = 'requested' AND NEW.challenge_state IN ('published', 'uncertain'))
   OR (OLD.challenge_state IN ('published', 'uncertain') AND NEW.challenge_state = 'abandoned')
 )
 OR NOT (
   (OLD.proof_state = NEW.proof_state)
   OR (OLD.proof_state = 'pending' AND NEW.proof_state IN ('approved', 'denied'))
 )
 OR (OLD.challenge_state = 'published' AND (
      OLD.challenge_file_inode != NEW.challenge_file_inode
   OR OLD.challenge_file_bytes != NEW.challenge_file_bytes
   OR OLD.challenge_file_uid != NEW.challenge_file_uid
   OR OLD.challenge_file_gid != NEW.challenge_file_gid
   OR OLD.challenge_file_mode != NEW.challenge_file_mode
   OR OLD.challenge_mount_id != NEW.challenge_mount_id
   OR OLD.challenge_device_major != NEW.challenge_device_major
   OR OLD.challenge_device_minor != NEW.challenge_device_minor
   OR OLD.challenge_statx_device_major != NEW.challenge_statx_device_major
   OR OLD.challenge_statx_device_minor != NEW.challenge_statx_device_minor
   OR OLD.challenge_fs_type_magic != NEW.challenge_fs_type_magic
   OR OLD.challenge_filesystem_id != NEW.challenge_filesystem_id
   OR OLD.challenge_filesystem_instance_digest != NEW.challenge_filesystem_instance_digest
   OR OLD.challenge_read_only != NEW.challenge_read_only OR OLD.challenge_no_copy != NEW.challenge_no_copy
 ))
 OR (OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (
    SELECT 1 FROM capacity_reservations AS reservation
    JOIN connection_intents AS intent ON intent.id = reservation.intent_id
    JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id
    JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id
    JOIN connection_request_authorizations AS auth
      ON auth.connection_id = NEW.connection_id AND auth.scale_set_id = NEW.scale_set_id
     AND auth.request_id = NEW.actual_request_id
    WHERE NEW.native_job_id IS NOT NULL AND reservation.id = NEW.reservation_id AND reservation.intent_id = NEW.intent_id
      AND reservation.connection_id = NEW.connection_id AND reservation.state = 'reserved'
      AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0
      AND intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id
      AND intent.runner_request_id = NEW.actual_request_id AND intent.cleanup_proven = 0
      AND intent.runner_completed = 0 AND jit.witness_request_id = NEW.witness_request_id
      AND jit.started_request_id = NEW.actual_request_id AND jit.runner_id = NEW.runner_id
      AND jit.runner_name = NEW.runner_name AND jit.state = 'started'
      AND jit.completed_request_id IS NULL AND auth.acquire_state = 'acquired'
      AND auth.started_event_digest = NEW.started_event_digest
      AND auth.started_runner_id = NEW.runner_id AND auth.started_runner_name = NEW.runner_name
      AND auth.started_job_id IS NEW.native_job_id AND auth.completed_event_digest IS NULL
      AND state.resources_bound = 1 AND state.instance_binding_digest = NEW.instance_binding_digest
      AND state.delivery_state = 'delivered' AND state.storage_cleanup_proven = 0
 ))
BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END";

/// The complete set of proof guards for schema version 27.
/// The host replaces this set in one database transaction.
pub(in crate::journal::schema::execution_authorization::ddl) const V27_PROOF_TRANSITION_GUARDS:
    &[(&str, &str)] = &[
    (
        "execution_proof_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_transition_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.intent_id != NEW.intent_id OR OLD.reservation_id != NEW.reservation_id OR OLD.connection_id != NEW.connection_id OR OLD.scale_set_id != NEW.scale_set_id OR OLD.witness_request_id != NEW.witness_request_id OR OLD.binding_digest != NEW.binding_digest OR OLD.generation != NEW.generation OR OLD.policy_digest != NEW.policy_digest BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_identity_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_identity_transition_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest OR OLD.runner_id != NEW.runner_id OR OLD.runner_name != NEW.runner_name OR OLD.runner_container_id != NEW.runner_container_id OR OLD.dind_container_id != NEW.dind_container_id OR OLD.instance_binding_digest != NEW.instance_binding_digest OR OLD.challenge != NEW.challenge OR OLD.channel_digest != NEW.channel_digest OR OLD.channel_json != NEW.channel_json OR OLD.challenge_json != NEW.challenge_json OR OLD.challenge_digest != NEW.challenge_digest BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_source_immutable_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_source_immutable_guard BEFORE UPDATE ON connection_execution_proofs WHEN (OLD.source_digest IS NOT NULL AND OLD.source_digest != NEW.source_digest) OR (OLD.source_json IS NOT NULL AND OLD.source_json != NEW.source_json) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_challenge_state_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_challenge_state_guard BEFORE UPDATE ON connection_execution_proofs WHEN NOT ((OLD.challenge_state = NEW.challenge_state) OR (OLD.challenge_state = 'not_requested' AND NEW.challenge_state = 'requested') OR (OLD.challenge_state = 'requested' AND NEW.challenge_state IN ('published', 'uncertain')) OR (OLD.challenge_state IN ('published', 'uncertain') AND NEW.challenge_state = 'abandoned')) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_state_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_state_guard BEFORE UPDATE ON connection_execution_proofs WHEN NOT ((OLD.proof_state = NEW.proof_state) OR (OLD.proof_state = 'pending' AND NEW.proof_state IN ('approved', 'denied'))) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_published_file_guard",
        r"CREATE TRIGGER IF NOT EXISTS execution_proof_published_file_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.challenge_state = 'published' AND (OLD.challenge_file_inode != NEW.challenge_file_inode OR OLD.challenge_file_bytes != NEW.challenge_file_bytes OR OLD.challenge_file_uid != NEW.challenge_file_uid OR OLD.challenge_file_gid != NEW.challenge_file_gid OR OLD.challenge_file_mode != NEW.challenge_file_mode OR OLD.challenge_mount_id != NEW.challenge_mount_id OR OLD.challenge_device_major != NEW.challenge_device_major OR OLD.challenge_device_minor != NEW.challenge_device_minor OR OLD.challenge_statx_device_major != NEW.challenge_statx_device_major OR OLD.challenge_statx_device_minor != NEW.challenge_statx_device_minor OR OLD.challenge_fs_type_magic != NEW.challenge_fs_type_magic OR OLD.challenge_filesystem_id != NEW.challenge_filesystem_id OR OLD.challenge_filesystem_instance_digest != NEW.challenge_filesystem_instance_digest OR OLD.challenge_read_only != NEW.challenge_read_only OR OLD.challenge_no_copy != NEW.challenge_no_copy) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_job_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_job_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NEW.native_job_id IS NULL BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_reservation_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_reservation_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation WHERE reservation.id = NEW.reservation_id AND reservation.intent_id = NEW.intent_id AND reservation.connection_id = NEW.connection_id AND reservation.state = 'reserved' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_intent_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_intent_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (SELECT 1 FROM connection_intents AS intent WHERE intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id AND intent.runner_request_id = NEW.actual_request_id AND intent.cleanup_proven = 0 AND intent.runner_completed = 0) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_jit_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_jit_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = NEW.intent_id AND jit.connection_id = NEW.connection_id AND jit.witness_request_id = NEW.witness_request_id AND jit.started_request_id = NEW.actual_request_id AND jit.runner_id = NEW.runner_id AND jit.runner_name = NEW.runner_name AND jit.state = 'started' AND jit.completed_request_id IS NULL) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_auth_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_auth_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS auth WHERE auth.connection_id = NEW.connection_id AND auth.scale_set_id = NEW.scale_set_id AND auth.request_id = NEW.actual_request_id AND auth.acquire_state = 'acquired' AND auth.started_event_digest = NEW.started_event_digest AND auth.started_runner_id = NEW.runner_id AND auth.started_runner_name = NEW.runner_name AND auth.started_job_id IS NEW.native_job_id AND auth.completed_event_digest IS NULL) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_approval_state_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_approval_state_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.proof_state = 'pending' AND NEW.proof_state = 'approved' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = NEW.reservation_id AND state.resources_bound = 1 AND state.instance_binding_digest = NEW.instance_binding_digest AND state.delivery_state = 'delivered' AND state.storage_cleanup_proven = 0) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
];

/// Each proof guard checks a small group of fields.
pub(in crate::journal::schema::execution_authorization::ddl) const PROOF_TRANSITION_GUARDS: &[(
    &str,
    &str,
)] = &[
    V27_PROOF_TRANSITION_GUARDS[0],
    V27_PROOF_TRANSITION_GUARDS[2],
    V27_PROOF_TRANSITION_GUARDS[3],
    V27_PROOF_TRANSITION_GUARDS[4],
    (
        "execution_proof_identity_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_identity_transition_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest OR OLD.runner_id != NEW.runner_id OR OLD.runner_name != NEW.runner_name BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_identity_runtime_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_identity_runtime_transition_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.runner_container_id != NEW.runner_container_id OR OLD.dind_container_id != NEW.dind_container_id OR OLD.instance_binding_digest != NEW.instance_binding_digest OR OLD.challenge != NEW.challenge BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_identity_channel_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_identity_channel_transition_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.channel_digest != NEW.channel_digest OR OLD.channel_json != NEW.channel_json OR OLD.challenge_json != NEW.challenge_json OR OLD.challenge_digest != NEW.challenge_digest BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_published_file_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_published_file_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.challenge_state = 'published' AND (OLD.challenge_file_inode != NEW.challenge_file_inode OR OLD.challenge_file_bytes != NEW.challenge_file_bytes OR OLD.challenge_file_uid != NEW.challenge_file_uid OR OLD.challenge_file_gid != NEW.challenge_file_gid OR OLD.challenge_file_mode != NEW.challenge_file_mode) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_published_mount_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_published_mount_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.challenge_state = 'published' AND (OLD.challenge_mount_id != NEW.challenge_mount_id OR OLD.challenge_device_major != NEW.challenge_device_major OR OLD.challenge_device_minor != NEW.challenge_device_minor OR OLD.challenge_statx_device_major != NEW.challenge_statx_device_major OR OLD.challenge_statx_device_minor != NEW.challenge_statx_device_minor) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    (
        "execution_proof_published_filesystem_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_published_filesystem_guard BEFORE UPDATE ON connection_execution_proofs WHEN OLD.challenge_state = 'published' AND (OLD.challenge_fs_type_magic != NEW.challenge_fs_type_magic OR OLD.challenge_filesystem_id != NEW.challenge_filesystem_id OR OLD.challenge_filesystem_instance_digest != NEW.challenge_filesystem_instance_digest OR OLD.challenge_read_only != NEW.challenge_read_only OR OLD.challenge_no_copy != NEW.challenge_no_copy) BEGIN SELECT RAISE(ABORT, 'invalid execution proof transition'); END",
    ),
    V27_PROOF_TRANSITION_GUARDS[6],
    V27_PROOF_TRANSITION_GUARDS[7],
    V27_PROOF_TRANSITION_GUARDS[8],
    V27_PROOF_TRANSITION_GUARDS[9],
    V27_PROOF_TRANSITION_GUARDS[10],
    V27_PROOF_TRANSITION_GUARDS[11],
];

pub(in crate::journal::schema::execution_authorization::ddl) async fn bootstrap_proof_transition_guards(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), crate::error::HostError> {
    use super::super::super::super::sql_normalization::normalize;
    use super::proof_insert::trigger_sql;

    let existing = trigger_set_sql(connection, PROOF_TRANSITION_GUARDS).await?;
    let v27 = trigger_set_sql(connection, V27_PROOF_TRANSITION_GUARDS).await?;
    let legacy = trigger_sql(connection, "execution_proof_transition_guard").await?;
    let exact = |actual: &[Option<String>], expected: &[(&str, &str)]| {
        actual.len() == expected.len()
            && actual.iter().zip(expected).all(|(actual, (_, expected))| {
                actual
                    .as_deref()
                    .is_some_and(|actual| normalize(actual) == normalize(expected))
            })
    };
    let current_exact = exact(&existing, PROOF_TRANSITION_GUARDS);
    if current_exact {
        return Ok(());
    }

    let mut v27_exact = exact(&v27, V27_PROOF_TRANSITION_GUARDS);
    if v27_exact {
        for (name, _) in PROOF_TRANSITION_GUARDS {
            if !V27_PROOF_TRANSITION_GUARDS
                .iter()
                .any(|(old_name, _)| old_name == name)
                && trigger_sql(connection, name).await?.is_some()
            {
                v27_exact = false;
                break;
            }
        }
    }
    let legacy_exact = legacy
        .as_deref()
        .is_some_and(|sql| normalize(sql) == normalize(LEGACY_PROOF_TRANSITION_GUARD));
    let no_other_current = if legacy_exact {
        let mut clear = true;
        for (name, _) in PROOF_TRANSITION_GUARDS.iter().skip(1) {
            if trigger_sql(connection, name).await?.is_some() {
                clear = false;
            }
        }
        clear
    } else {
        false
    };

    if v27_exact {
        for (name, _) in V27_PROOF_TRANSITION_GUARDS {
            connection
                .execute(&format!("DROP TRIGGER {name}"), ())
                .await
                .map_err(|_| crate::error::HostError::Journal)?;
        }
    } else if legacy_exact && no_other_current {
        connection
            .execute("DROP TRIGGER execution_proof_transition_guard", ())
            .await
            .map_err(|_| crate::error::HostError::Journal)?;
    } else if legacy.is_some() || existing.iter().any(Option::is_some) || table_existed {
        return Err(crate::error::HostError::Journal);
    }
    for (_, statement) in PROOF_TRANSITION_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| crate::error::HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn trigger_set_sql(
    connection: &turso::Connection,
    triggers: &[(&str, &str)],
) -> Result<Vec<Option<String>>, crate::error::HostError> {
    let mut result = Vec::with_capacity(triggers.len());
    for (name, _) in triggers {
        result.push(trigger_sql(connection, name).await?);
    }
    Ok(result)
}
