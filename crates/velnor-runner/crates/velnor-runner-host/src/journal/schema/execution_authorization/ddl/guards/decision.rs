const POLICY_IMMUTABLE: &str = "CREATE TRIGGER IF NOT EXISTS execution_policy_immutable BEFORE UPDATE ON connection_job_authority_policies BEGIN SELECT RAISE(ABORT, 'immutable execution policy'); END";
const POLICY_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_policy_no_delete BEFORE DELETE ON connection_job_authority_policies BEGIN SELECT RAISE(ABORT, 'immutable execution policy'); END";
const ACTIVE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS execution_active_guard BEFORE UPDATE ON connection_active_job_authority WHEN OLD.connection_id != NEW.connection_id OR NEW.generation <= OLD.generation OR NOT EXISTS (SELECT 1 FROM connection_job_authority_policies AS policy WHERE policy.connection_id = NEW.connection_id AND policy.generation = NEW.generation AND policy.policy_digest = NEW.policy_digest) BEGIN SELECT RAISE(ABORT, 'invalid active execution policy'); END";
const ACTIVE_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS execution_active_no_delete BEFORE DELETE ON connection_active_job_authority BEGIN SELECT RAISE(ABORT, 'active execution policy required'); END";
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
pub(in crate::journal::schema::execution_authorization::ddl) const LEGACY_DECISION_TRANSITION_GUARD: &str = r"CREATE TRIGGER IF NOT EXISTS execution_decision_transition_guard
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
pub(in crate::journal::schema::execution_authorization::ddl) const DECISION_TRANSITION_GUARDS:
    &[(&str, &str)] = &[
    (
        "execution_decision_transition_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_transition_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.intent_id != NEW.intent_id OR OLD.reservation_id != NEW.reservation_id OR OLD.connection_id != NEW.connection_id OR OLD.scale_set_id != NEW.scale_set_id BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_identity_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.witness_request_id != NEW.witness_request_id OR OLD.actual_request_id != NEW.actual_request_id OR OLD.challenge != NEW.challenge OR OLD.generation != NEW.generation BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_receipt_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_receipt_identity_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.policy_digest != NEW.policy_digest OR OLD.request_digest != NEW.request_digest OR OLD.allow != NEW.allow OR OLD.decision_digest != NEW.decision_digest OR OLD.decision_json != NEW.decision_json BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_state_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_state_guard BEFORE UPDATE ON connection_execution_decisions WHEN NOT ((OLD.delivery_state = 'prepared' AND NEW.delivery_state IN ('requested', 'abandoned')) OR (OLD.delivery_state = 'requested' AND NEW.delivery_state IN ('delivered', 'uncertain'))) BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_delivered_file_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_delivered_file_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.delivery_state = 'delivered' AND (OLD.decision_file_path IS NOT NEW.decision_file_path OR OLD.decision_file_inode IS NOT NEW.decision_file_inode OR OLD.decision_file_bytes IS NOT NEW.decision_file_bytes OR OLD.decision_file_uid IS NOT NEW.decision_file_uid) BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_delivered_owner_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_delivered_owner_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.delivery_state = 'delivered' AND (OLD.decision_file_gid IS NOT NEW.decision_file_gid OR OLD.decision_file_mode IS NOT NEW.decision_file_mode OR OLD.decision_mount_id IS NOT NEW.decision_mount_id OR OLD.decision_device_major IS NOT NEW.decision_device_major) BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_delivered_device_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_delivered_device_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.delivery_state = 'delivered' AND (OLD.decision_device_minor IS NOT NEW.decision_device_minor OR OLD.decision_statx_device_major IS NOT NEW.decision_statx_device_major OR OLD.decision_statx_device_minor IS NOT NEW.decision_statx_device_minor OR OLD.decision_fs_type_magic IS NOT NEW.decision_fs_type_magic) BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
    (
        "execution_decision_delivered_filesystem_guard",
        "CREATE TRIGGER IF NOT EXISTS execution_decision_delivered_filesystem_guard BEFORE UPDATE ON connection_execution_decisions WHEN OLD.delivery_state = 'delivered' AND (OLD.decision_filesystem_id IS NOT NEW.decision_filesystem_id OR OLD.decision_filesystem_instance_digest IS NOT NEW.decision_filesystem_instance_digest OR OLD.decision_read_only IS NOT NEW.decision_read_only OR OLD.decision_no_copy IS NOT NEW.decision_no_copy) BEGIN SELECT RAISE(ABORT, 'invalid execution decision transition'); END",
    ),
];

pub(in crate::journal::schema::execution_authorization::ddl) async fn bootstrap_decision_transition_guards(
    connection: &turso::Connection,
    table_existed: bool,
) -> Result<(), crate::error::HostError> {
    use super::super::super::super::sql_normalization::normalize;
    use super::proof_insert::trigger_sql;
    use super::proof_transition::trigger_set_sql;
    use crate::error::HostError;

    let existing = trigger_set_sql(connection, DECISION_TRANSITION_GUARDS).await?;
    let current_exact = existing.len() == DECISION_TRANSITION_GUARDS.len()
        && existing
            .iter()
            .zip(DECISION_TRANSITION_GUARDS)
            .all(|(actual, (_, expected))| {
                actual
                    .as_deref()
                    .is_some_and(|actual| normalize(actual) == normalize(expected))
            });
    if current_exact {
        return Ok(());
    }

    let legacy = trigger_sql(connection, "execution_decision_transition_guard").await?;
    let legacy_exact = legacy
        .as_deref()
        .is_some_and(|actual| normalize(actual) == normalize(LEGACY_DECISION_TRANSITION_GUARD));
    let no_extras = if legacy_exact {
        let mut clear = true;
        for (name, _) in DECISION_TRANSITION_GUARDS.iter().skip(1) {
            if trigger_sql(connection, name).await?.is_some() {
                clear = false;
                break;
            }
        }
        clear
    } else {
        false
    };

    if legacy_exact && no_extras {
        connection
            .execute("DROP TRIGGER execution_decision_transition_guard", ())
            .await
            .map_err(|_| HostError::Journal)?;
    } else if existing.iter().any(Option::is_some) || table_existed {
        return Err(HostError::Journal);
    }
    for (_, statement) in DECISION_TRANSITION_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
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
pub(in crate::journal::schema::execution_authorization::ddl) const ACTUAL_REQUEST_INDEX: &str = "CREATE UNIQUE INDEX IF NOT EXISTS execution_actual_request ON connection_execution_proofs(connection_id, scale_set_id, actual_request_id) WHERE actual_request_id IS NOT NULL";

pub(in crate::journal::schema::execution_authorization::ddl) const OBJECTS: &[(
    &str,
    &str,
    &str,
)] = &[
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
