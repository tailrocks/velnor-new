//! Immutable guards for execution-authority records.

const POLICY_IMMUTABLE: &str = "CREATE TRIGGER IF NOT EXISTS execution_policy_immutable BEFORE UPDATE ON connection_job_authority_policies BEGIN SELECT RAISE(ABORT, 'immutable execution policy'); END";
const POLICY_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_policy_no_delete BEFORE DELETE ON connection_job_authority_policies BEGIN SELECT RAISE(ABORT, 'immutable execution policy'); END";
const ACTIVE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_active_guard BEFORE UPDATE ON connection_active_job_authority WHEN OLD.connection_id != NEW.connection_id OR NEW.generation <= OLD.generation OR NOT EXISTS (SELECT 1 FROM connection_job_authority_policies AS policy WHERE policy.connection_id = NEW.connection_id AND policy.generation = NEW.generation AND policy.policy_digest = NEW.policy_digest) BEGIN SELECT RAISE(ABORT, 'invalid active execution policy'); END";
const ACTIVE_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_active_no_delete BEFORE DELETE ON connection_active_job_authority BEGIN SELECT RAISE(ABORT, 'active execution policy required'); END";
pub(super) const LEGACY_PROOF_INSERT_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_proof_insert_guard
BEFORE INSERT ON connection_execution_proofs
WHEN NEW.proof_state != 'pending' OR NEW.challenge_state != 'not_requested'
 OR NOT EXISTS (
    SELECT 1 FROM connection_active_job_authority AS active
    WHERE active.connection_id = NEW.connection_id
      AND active.generation = NEW.generation AND active.policy_digest = NEW.policy_digest
 )
 OR NOT EXISTS (
    SELECT 1
    FROM capacity_reservations AS reservation
    JOIN connection_intents AS intent ON intent.id = reservation.intent_id
    JOIN connection_request_reservations AS link
      ON link.reservation_id = reservation.id AND link.connection_id = reservation.connection_id
    JOIN connection_runner_jits AS jit
      ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id
    JOIN capacity_reservation_provisioning AS provisioning
      ON provisioning.reservation_id = reservation.id AND provisioning.operation_id = intent.id
    JOIN capacity_reservation_provisioning_state AS state
      ON state.reservation_id = reservation.id
    JOIN capacity_reservation_provisioning_publications AS publication
      ON publication.reservation_id = reservation.id
    JOIN connection_request_authorizations AS witness
      ON witness.connection_id = NEW.connection_id AND witness.scale_set_id = NEW.scale_set_id
     AND witness.request_id = NEW.witness_request_id
    WHERE reservation.id = NEW.reservation_id AND reservation.intent_id = NEW.intent_id
      AND reservation.connection_id = NEW.connection_id AND reservation.state = 'reserved'
      AND reservation.binding_digest = NEW.binding_digest
      AND reservation.engine_id = NEW.engine_id
      AND reservation.docker_root_digest = NEW.docker_root_digest
      AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0
      AND intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id
      AND intent.scale_set_id = NEW.scale_set_id AND intent.kind = 'launch'
      AND intent.state = 'pending' AND intent.cleanup_proven = 0
      AND intent.runner_completed = 0 AND intent.runner_request_id IS NULL
      AND intent.runner_name = NEW.runner_name AND intent.github_runner_id = CAST(NEW.runner_id AS TEXT)
      AND jit.scale_set_id = NEW.scale_set_id AND jit.witness_request_id = NEW.witness_request_id
      AND jit.runner_id = NEW.runner_id AND jit.runner_name = NEW.runner_name AND jit.state = 'bound'
      AND link.request_id = NEW.witness_request_id AND link.scale_set_id = NEW.scale_set_id
      AND link.binding_digest = NEW.binding_digest AND witness.acquire_state = 'acquired'
      AND provisioning.engine_id = NEW.engine_id AND provisioning.docker_root_digest = NEW.docker_root_digest
      AND state.resources_bound = 1 AND state.bootstrap_start_state = 'started'
      AND state.instance_binding_digest = NEW.instance_binding_digest AND state.jit_state = 'bound'
      AND state.publication_state = 'published' AND state.delivery_state = 'not_requested'
      AND state.storage_cleanup_proven = 0
      AND publication.runner_id = NEW.runner_id AND publication.runner_name = NEW.runner_name
      AND publication.runner_scale_set_id = NEW.scale_set_id
 )
BEGIN SELECT RAISE(ABORT, 'invalid execution proof preparation'); END";

pub(super) const PROOF_INSERT_GUARDS: &[(&str, &str)] = &[
    (
        "execution_proof_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_guard BEFORE INSERT ON connection_execution_proofs WHEN NEW.proof_state != 'pending' OR NEW.challenge_state != 'not_requested' BEGIN SELECT RAISE(ABORT, 'invalid execution proof state'); END",
    ),
    (
        "execution_proof_insert_active_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_active_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM connection_active_job_authority AS active WHERE active.connection_id = NEW.connection_id AND active.generation = NEW.generation AND active.policy_digest = NEW.policy_digest) BEGIN SELECT RAISE(ABORT, 'invalid active execution policy'); END",
    ),
    (
        "execution_proof_insert_reservation_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_reservation_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation WHERE reservation.id = NEW.reservation_id AND reservation.intent_id = NEW.intent_id AND reservation.connection_id = NEW.connection_id AND reservation.state = 'reserved' AND reservation.binding_digest = NEW.binding_digest AND reservation.engine_id = NEW.engine_id AND reservation.docker_root_digest = NEW.docker_root_digest AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0) BEGIN SELECT RAISE(ABORT, 'invalid execution reservation'); END",
    ),
    (
        "execution_proof_insert_intent_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_intent_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM connection_intents AS intent WHERE intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id AND intent.scale_set_id = NEW.scale_set_id AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.runner_completed = 0 AND intent.runner_request_id IS NULL AND intent.runner_name = NEW.runner_name AND intent.github_runner_id = CAST(NEW.runner_id AS TEXT)) BEGIN SELECT RAISE(ABORT, 'invalid execution intent'); END",
    ),
    (
        "execution_proof_insert_request_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_request_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM connection_request_reservations AS link WHERE link.reservation_id = NEW.reservation_id AND link.connection_id = NEW.connection_id AND link.request_id = NEW.witness_request_id AND link.scale_set_id = NEW.scale_set_id AND link.binding_digest = NEW.binding_digest) BEGIN SELECT RAISE(ABORT, 'invalid execution request link'); END",
    ),
    (
        "execution_proof_insert_jit_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_jit_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = NEW.intent_id AND jit.connection_id = NEW.connection_id AND jit.scale_set_id = NEW.scale_set_id AND jit.witness_request_id = NEW.witness_request_id AND jit.runner_id = NEW.runner_id AND jit.runner_name = NEW.runner_name AND jit.state = 'bound') BEGIN SELECT RAISE(ABORT, 'invalid execution JIT identity'); END",
    ),
    (
        "execution_proof_insert_witness_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_witness_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS witness WHERE witness.connection_id = NEW.connection_id AND witness.scale_set_id = NEW.scale_set_id AND witness.request_id = NEW.witness_request_id AND witness.acquire_state = 'acquired') BEGIN SELECT RAISE(ABORT, 'invalid acquired request witness'); END",
    ),
    (
        "execution_proof_insert_provisioning_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_provisioning_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS provisioning WHERE provisioning.reservation_id = NEW.reservation_id AND provisioning.operation_id = NEW.intent_id AND provisioning.engine_id = NEW.engine_id AND provisioning.docker_root_digest = NEW.docker_root_digest) BEGIN SELECT RAISE(ABORT, 'invalid execution provisioning'); END",
    ),
    (
        "execution_proof_insert_state_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_state_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = NEW.reservation_id AND state.resources_bound = 1 AND state.bootstrap_start_state = 'started' AND state.instance_binding_digest = NEW.instance_binding_digest AND state.jit_state = 'bound' AND state.publication_state = 'published' AND state.delivery_state = 'not_requested' AND state.storage_cleanup_proven = 0) BEGIN SELECT RAISE(ABORT, 'invalid execution resource state'); END",
    ),
    (
        "execution_proof_insert_publication_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_proof_insert_publication_guard BEFORE INSERT ON connection_execution_proofs WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_publications AS publication WHERE publication.reservation_id = NEW.reservation_id AND publication.runner_id = NEW.runner_id AND publication.runner_name = NEW.runner_name AND publication.runner_scale_set_id = NEW.scale_set_id) BEGIN SELECT RAISE(ABORT, 'invalid execution publication'); END",
    ),
];

pub(super) async fn bootstrap_proof_insert_guards(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), crate::error::HostError> {
    use super::super::super::sql_normalization::normalize;
    use crate::error::HostError;

    let mut existing = Vec::with_capacity(PROOF_INSERT_GUARDS.len());
    for (name, _) in PROOF_INSERT_GUARDS {
        existing.push(trigger_sql(connection, name).await?);
    }
    let legacy = trigger_sql(connection, "execution_proof_insert_guard").await?;
    let extra_present = existing.iter().skip(1).any(Option::is_some);
    let main_is_current = existing
        .first()
        .and_then(Option::as_deref)
        .is_some_and(|sql| normalize(sql) == normalize(PROOF_INSERT_GUARDS[0].1));
    let main_is_legacy = legacy
        .as_deref()
        .is_some_and(|sql| normalize(sql) == normalize(LEGACY_PROOF_INSERT_GUARD));

    if main_is_legacy {
        if extra_present {
            return Err(HostError::Journal);
        }
        connection
            .execute("DROP TRIGGER execution_proof_insert_guard", ())
            .await
            .map_err(|_| HostError::Journal)?;
    } else if main_is_current {
        if existing.iter().any(Option::is_none) {
            return Err(HostError::Journal);
        }
        for ((name, expected), actual) in PROOF_INSERT_GUARDS.iter().zip(existing.iter()) {
            if actual
                .as_deref()
                .is_none_or(|sql| normalize(sql) != normalize(expected))
            {
                let _ = name;
                return Err(HostError::Journal);
            }
        }
        return Ok(());
    } else if legacy.is_some() || extra_present || table_existed {
        return Err(HostError::Journal);
    }

    for (_, statement) in PROOF_INSERT_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn trigger_sql(
    connection: &turso::Connection,
    name: &str,
) -> Result<Option<String>, crate::error::HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
            [name],
        )
        .await
        .map_err(|_| crate::error::HostError::Journal)?;
    let Some(row) = rows
        .next()
        .await
        .map_err(|_| crate::error::HostError::Journal)?
    else {
        return Ok(None);
    };
    row.get::<String>(0)
        .map(Some)
        .map_err(|_| crate::error::HostError::Journal)
}

pub(super) const LEGACY_PROOF_TRANSITION_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_proof_transition_guard
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

pub(super) const PROOF_TRANSITION_GUARDS: &[(&str, &str)] = &[
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

pub(super) async fn bootstrap_proof_transition_guards(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), crate::error::HostError> {
    let existing = trigger_set_sql(connection, PROOF_TRANSITION_GUARDS).await?;
    let legacy = trigger_sql(connection, "execution_proof_transition_guard").await?;
    let extras_present = existing.iter().skip(1).any(Option::is_some);
    let legacy_exact = legacy.as_deref().is_some_and(|sql| {
        super::super::super::sql_normalization::normalize(sql)
            == super::super::super::sql_normalization::normalize(LEGACY_PROOF_TRANSITION_GUARD)
    });
    let current_exact = existing
        .first()
        .and_then(Option::as_deref)
        .is_some_and(|sql| {
            super::super::super::sql_normalization::normalize(sql)
                == super::super::super::sql_normalization::normalize(PROOF_TRANSITION_GUARDS[0].1)
        });
    if legacy_exact {
        if extras_present {
            return Err(crate::error::HostError::Journal);
        }
        connection
            .execute("DROP TRIGGER execution_proof_transition_guard", ())
            .await
            .map_err(|_| crate::error::HostError::Journal)?;
    } else if current_exact {
        if existing.iter().any(Option::is_none)
            || existing
                .iter()
                .zip(PROOF_TRANSITION_GUARDS)
                .any(|(actual, (_, expected))| {
                    actual.as_deref().is_none_or(|sql| {
                        super::super::super::sql_normalization::normalize(sql)
                            != super::super::super::sql_normalization::normalize(expected)
                    })
                })
        {
            return Err(crate::error::HostError::Journal);
        }
        return Ok(());
    } else if legacy.is_some() || extras_present || table_existed {
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

async fn trigger_set_sql(
    connection: &turso::Connection,
    triggers: &[(&str, &str)],
) -> Result<Vec<Option<String>>, crate::error::HostError> {
    let mut result = Vec::with_capacity(triggers.len());
    for (name, _) in triggers {
        result.push(trigger_sql(connection, name).await?);
    }
    Ok(result)
}

const PROOF_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_proof_no_delete BEFORE DELETE ON connection_execution_proofs BEGIN SELECT RAISE(ABORT, 'immutable execution proof'); END";
const DECISION_INSERT_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_decision_insert_guard
BEFORE INSERT ON connection_execution_decisions
WHEN NEW.delivery_state != 'prepared' OR NOT EXISTS (
    SELECT 1 FROM connection_execution_proofs AS proof
    WHERE proof.intent_id = NEW.intent_id AND proof.reservation_id = NEW.reservation_id
      AND proof.connection_id = NEW.connection_id AND proof.scale_set_id = NEW.scale_set_id
      AND proof.witness_request_id = NEW.witness_request_id
      AND proof.actual_request_id = NEW.actual_request_id AND proof.challenge = NEW.challenge
      AND proof.generation = NEW.generation AND proof.policy_digest = NEW.policy_digest
      AND proof.proof_state IN ('approved', 'denied')
)
BEGIN SELECT RAISE(ABORT, 'invalid execution decision'); END";
const DECISION_TRANSITION_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_decision_transition_guard
BEFORE UPDATE ON connection_execution_decisions
WHEN OLD.intent_id != NEW.intent_id OR OLD.reservation_id != NEW.reservation_id
 OR OLD.connection_id != NEW.connection_id OR OLD.scale_set_id != NEW.scale_set_id
 OR OLD.witness_request_id != NEW.witness_request_id OR OLD.actual_request_id != NEW.actual_request_id
 OR OLD.challenge != NEW.challenge OR OLD.generation != NEW.generation
 OR OLD.policy_digest != NEW.policy_digest OR OLD.request_digest != NEW.request_digest
 OR OLD.allow != NEW.allow OR OLD.decision_digest != NEW.decision_digest
 OR OLD.decision_json != NEW.decision_json
 OR NOT ((OLD.delivery_state = 'prepared' AND NEW.delivery_state IN ('requested', 'abandoned'))
      OR (OLD.delivery_state = 'requested' AND NEW.delivery_state IN ('delivered', 'uncertain')))
 OR (OLD.delivery_state = 'delivered' AND (
      OLD.decision_file_path IS NOT NEW.decision_file_path
   OR OLD.decision_file_inode IS NOT NEW.decision_file_inode
   OR OLD.decision_file_bytes IS NOT NEW.decision_file_bytes
   OR OLD.decision_file_uid IS NOT NEW.decision_file_uid
   OR OLD.decision_file_gid IS NOT NEW.decision_file_gid
   OR OLD.decision_file_mode IS NOT NEW.decision_file_mode
   OR OLD.decision_mount_id IS NOT NEW.decision_mount_id
   OR OLD.decision_device_major IS NOT NEW.decision_device_major
   OR OLD.decision_device_minor IS NOT NEW.decision_device_minor
   OR OLD.decision_statx_device_major IS NOT NEW.decision_statx_device_major
   OR OLD.decision_statx_device_minor IS NOT NEW.decision_statx_device_minor
   OR OLD.decision_fs_type_magic IS NOT NEW.decision_fs_type_magic
   OR OLD.decision_filesystem_id IS NOT NEW.decision_filesystem_id
   OR OLD.decision_filesystem_instance_digest IS NOT NEW.decision_filesystem_instance_digest
   OR OLD.decision_read_only IS NOT NEW.decision_read_only
   OR OLD.decision_no_copy IS NOT NEW.decision_no_copy
 ))
BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END";
const DECISION_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_decision_no_delete BEFORE DELETE ON connection_execution_decisions BEGIN SELECT RAISE(ABORT, 'immutable execution decision'); END";
const LISTENER_CHALLENGE_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_listener_challenge_guard
BEFORE UPDATE OF delivery_state ON capacity_reservation_provisioning_state
WHEN NEW.delivery_state = 'requested' AND NOT EXISTS (
    SELECT 1 FROM connection_execution_proofs AS proof
    WHERE proof.reservation_id = NEW.reservation_id AND proof.challenge_state = 'published'
)
BEGIN SELECT RAISE(ABORT, 'source challenge must be published before listener delivery'); END";
const RESERVATION_RELEASE_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_reservation_release_guard
BEFORE UPDATE OF state ON capacity_reservations
WHEN NEW.state = 'released' AND EXISTS (
    SELECT 1 FROM connection_execution_proofs AS proof
    LEFT JOIN connection_execution_decisions AS decision ON decision.intent_id = proof.intent_id
    WHERE proof.reservation_id = NEW.id
      AND (proof.proof_state = 'pending'
        OR proof.challenge_state IN ('requested', 'uncertain')
        OR decision.delivery_state IN ('prepared', 'requested', 'uncertain')
        OR (proof.proof_state = 'approved' AND NOT EXISTS (
            SELECT 1 FROM connection_execution_decisions AS delivered
            WHERE delivered.intent_id = proof.intent_id AND delivered.delivery_state = 'delivered'
        )))
      AND NOT EXISTS (
        SELECT 1 FROM connection_intents AS intent
        JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = NEW.id
        WHERE intent.id = NEW.intent_id AND intent.connection_id = NEW.connection_id
          AND intent.cleanup_proven = 1 AND NEW.remote_cleanup_proven = 1
          AND NEW.local_cleanup_proven = 1 AND state.storage_cleanup_proven = 1
          AND EXISTS (
            SELECT 1 FROM capacity_reservation_metadata_settlements AS settlement
            WHERE settlement.reservation_id = NEW.id AND settlement.outcome = 'settled'
          )
      )
)
BEGIN SELECT RAISE(ABORT, 'execution authorization remains unresolved'); END";
const ACTUAL_REQUEST_INDEX: &str = "CREATE UNIQUE INDEX IF NOT EXISTS execution_actual_request ON connection_execution_proofs(connection_id, scale_set_id, actual_request_id) WHERE actual_request_id IS NOT NULL";

pub(super) const OBJECTS: &[(&str, &str, &str)] = &[
    ("trigger", "execution_policy_immutable", POLICY_IMMUTABLE),
    ("trigger", "execution_policy_no_delete", POLICY_NO_DELETE),
    ("trigger", "execution_active_guard", ACTIVE_GUARD),
    ("trigger", "execution_active_no_delete", ACTIVE_NO_DELETE),
    ("trigger", "execution_proof_no_delete", PROOF_NO_DELETE),
    (
        "trigger",
        "execution_decision_insert_guard",
        DECISION_INSERT_GUARD,
    ),
    (
        "trigger",
        "execution_decision_transition_guard",
        DECISION_TRANSITION_GUARD,
    ),
    (
        "trigger",
        "execution_decision_no_delete",
        DECISION_NO_DELETE,
    ),
    (
        "trigger",
        "execution_listener_challenge_guard",
        LISTENER_CHALLENGE_GUARD,
    ),
    (
        "trigger",
        "execution_reservation_release_guard",
        RESERVATION_RELEASE_GUARD,
    ),
    ("index", "execution_actual_request", ACTUAL_REQUEST_INDEX),
];
