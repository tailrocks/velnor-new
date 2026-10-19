pub(in crate::journal::schema::execution_authorization::ddl) const LEGACY_PROOF_INSERT_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_proof_insert_guard
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

pub(in crate::journal::schema::execution_authorization::ddl) const PROOF_INSERT_GUARDS: &[(
    &str,
    &str,
)] = &[
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

pub(in crate::journal::schema::execution_authorization::ddl) async fn bootstrap_proof_insert_guards(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), crate::error::HostError> {
    use super::super::super::super::sql_normalization::normalize;
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

pub(super) async fn trigger_sql(
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
