//! Deny an exact Started request whose required source payload missed its deadline.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

#[cfg(test)]
#[path = "timeout_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartedRequestTimeoutResult {
    Enqueued,
    AlreadyEnqueued,
    NotEligible,
}

pub(super) async fn deny_started_request_timeout(
    scope: &ConnectionJournal,
    channel: &VerifiedProofChannelBinding,
    actual_request_id: i64,
) -> Result<StartedRequestTimeoutResult, HostError> {
    if actual_request_id <= 0
        || !channel.valid()
        || channel.connection_id() != scope.connection_id().as_str()
    {
        return Err(HostError::Identity);
    }
    let canonical_channel =
        String::from_utf8(channel.canonical_json()?).map_err(|_| HostError::Journal)?;
    let channel_digest = channel.computed_digest()?;
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let Some(started) = super::decision::load_started(
            connection,
            channel.operation_id(),
            channel.connection_id(),
        )
        .await?
        else {
            return Ok(StartedRequestTimeoutResult::NotEligible);
        };
        if started.request_id() != actual_request_id
            || started.jit_started_request_id() != Some(actual_request_id)
            || started.jit_witness_request_id() != channel.witness_request_id()
            || started.jit_runner_id() != Some(channel.runner_id())
            || started.jit_runner_name() != Some(channel.runner_name())
            || started.jit_state() != "started"
            || started.acquire_state() != "acquired"
            || started.runner_terminated()
        {
            return Ok(StartedRequestTimeoutResult::NotEligible);
        }
        let state = load_timeout_state(
            connection,
            channel,
            actual_request_id,
            &canonical_channel,
            &channel_digest,
        )
        .await?;
        match state {
            TimeoutState::AlreadyEnqueued => Ok(StartedRequestTimeoutResult::AlreadyEnqueued),
            TimeoutState::NotEligible => Ok(StartedRequestTimeoutResult::NotEligible),
            TimeoutState::Pending => {
                let changed = connection
                    .execute(
                        "UPDATE connection_execution_proofs SET proof_state = 'denied', challenge_state = 'abandoned', deny_code = 'proof_malformed' WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3 AND challenge_state = 'published' AND proof_state = 'pending' AND source_digest IS NULL AND source_json IS NULL AND actual_request_id IS NULL AND started_event_digest IS NULL AND NOT EXISTS (SELECT 1 FROM connection_execution_decisions WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3)",
                        (channel.operation_id(), channel.reservation_id(), channel.connection_id()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if changed != 1 {
                    return Ok(StartedRequestTimeoutResult::NotEligible);
                }
                connection
                    .execute(
                        "INSERT INTO connection_completion_cleanup (intent_id, connection_id, scale_set_id, runner_request_id, runner_id, runner_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        (channel.operation_id(), channel.connection_id(), channel.scale_set_id(), actual_request_id, channel.runner_id(), channel.runner_name()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                Ok(StartedRequestTimeoutResult::Enqueued)
            }
        }
    })
    .await
}

enum TimeoutState {
    Pending,
    AlreadyEnqueued,
    NotEligible,
}

async fn load_timeout_state(
    connection: &turso::Connection,
    channel: &VerifiedProofChannelBinding,
    actual_request_id: i64,
    canonical_channel: &str,
    channel_digest: &str,
) -> Result<TimeoutState, HostError> {
    let mut rows = connection
        .query(
            "SELECT p.channel_json, p.channel_digest, p.challenge_state, p.proof_state, p.source_digest, p.source_json, p.actual_request_id, p.started_event_digest, p.generation, p.policy_digest, p.binding_digest, p.engine_id, p.docker_root_digest, p.runner_id, p.runner_name, p.runner_container_id, p.dind_container_id, p.instance_binding_digest, r.state, r.remote_cleanup_proven, r.local_cleanup_proven, i.cleanup_proven, i.runner_completed, j.started_request_id, j.completed_request_id, j.state, s.resources_bound, s.bootstrap_start_state, s.jit_state, s.delivery_state, s.storage_cleanup_proven, auth.started_event_digest, auth.completed_event_digest, EXISTS (SELECT 1 FROM connection_execution_decisions AS d WHERE d.intent_id = p.intent_id AND d.reservation_id = p.reservation_id AND d.connection_id = p.connection_id), EXISTS (SELECT 1 FROM connection_completion_cleanup AS c WHERE c.intent_id = p.intent_id AND c.connection_id = p.connection_id AND c.scale_set_id = p.scale_set_id AND c.runner_request_id = ?1 AND c.runner_id = p.runner_id AND c.runner_name = p.runner_name), p.deny_code FROM connection_execution_proofs AS p JOIN capacity_reservations AS r ON r.id = p.reservation_id AND r.intent_id = p.intent_id AND r.connection_id = p.connection_id AND r.binding_digest = p.binding_digest JOIN connection_intents AS i ON i.id = p.intent_id AND i.connection_id = p.connection_id JOIN connection_runner_jits AS j ON j.intent_id = p.intent_id AND j.connection_id = p.connection_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id JOIN connection_request_authorizations AS auth ON auth.connection_id = p.connection_id AND auth.scale_set_id = p.scale_set_id AND auth.request_id = ?1 WHERE p.intent_id = ?2 AND p.reservation_id = ?3 AND p.connection_id = ?4 AND p.scale_set_id = ?5 AND p.witness_request_id = ?6",
            (actual_request_id, channel.operation_id(), channel.reservation_id(), channel.connection_id(), channel.scale_set_id(), channel.witness_request_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(TimeoutState::NotEligible);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let exact_channel = row.get::<String>(0).map_err(|_| HostError::Journal)? == canonical_channel
        && row.get::<String>(1).map_err(|_| HostError::Journal)? == channel_digest;
    let exact_identity = row.get::<String>(11).map_err(|_| HostError::Journal)?
        == channel.engine_id()
        && row.get::<String>(12).map_err(|_| HostError::Journal)? == channel.docker_root_digest()
        && row.get::<i64>(13).map_err(|_| HostError::Journal)? == channel.runner_id()
        && row.get::<String>(14).map_err(|_| HostError::Journal)? == channel.runner_name()
        && row.get::<String>(15).map_err(|_| HostError::Journal)? == channel.runner_container_id()
        && row.get::<String>(16).map_err(|_| HostError::Journal)? == channel.dind_container_id()
        && row.get::<String>(17).map_err(|_| HostError::Journal)?
            == channel.instance_binding_digest();
    let challenge_state: String = row.get(2).map_err(|_| HostError::Journal)?;
    let proof_state: String = row.get(3).map_err(|_| HostError::Journal)?;
    if exact_channel && exact_identity && already_enqueued(&row, &challenge_state, &proof_state)? {
        return Ok(TimeoutState::AlreadyEnqueued);
    }
    Ok(
        if pending_timeout_eligible(
            &row,
            actual_request_id,
            exact_channel,
            exact_identity,
            &challenge_state,
            &proof_state,
        )? {
            TimeoutState::Pending
        } else {
            TimeoutState::NotEligible
        },
    )
}

fn already_enqueued(
    row: &turso::Row,
    challenge_state: &str,
    proof_state: &str,
) -> Result<bool, HostError> {
    Ok(challenge_state == "abandoned"
        && proof_state == "denied"
        && row
            .get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<String>>(5)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<i64>>(6)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<String>>(7)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row.get::<i64>(8).map_err(|_| HostError::Journal)? > 0
        && valid_digest(&row.get::<String>(9).map_err(|_| HostError::Journal)?)
        && row.get::<i64>(33).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(34).map_err(|_| HostError::Journal)? == 1
        && row
            .get::<Option<String>>(35)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            == Some("proof_malformed"))
}

fn pending_timeout_eligible(
    row: &turso::Row,
    actual_request_id: i64,
    exact_channel: bool,
    exact_identity: bool,
    challenge_state: &str,
    proof_state: &str,
) -> Result<bool, HostError> {
    Ok(exact_channel
        && exact_identity
        && challenge_state == "published"
        && proof_state == "pending"
        && row
            .get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<String>>(5)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<i64>>(6)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row
            .get::<Option<String>>(7)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row.get::<i64>(8).map_err(|_| HostError::Journal)? > 0
        && valid_digest(&row.get::<String>(9).map_err(|_| HostError::Journal)?)
        && row.get::<String>(18).map_err(|_| HostError::Journal)? == "reserved"
        && row.get::<i64>(19).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(20).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(21).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(22).map_err(|_| HostError::Journal)? == 0
        && row.get::<Option<i64>>(23).map_err(|_| HostError::Journal)? == Some(actual_request_id)
        && row
            .get::<Option<i64>>(24)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row.get::<String>(25).map_err(|_| HostError::Journal)? == "started"
        && row.get::<i64>(26).map_err(|_| HostError::Journal)? == 1
        && row.get::<String>(27).map_err(|_| HostError::Journal)? == "started"
        && row.get::<String>(28).map_err(|_| HostError::Journal)? == "bound"
        && row.get::<String>(29).map_err(|_| HostError::Journal)? == "delivered"
        && row.get::<i64>(30).map_err(|_| HostError::Journal)? == 0
        && row
            .get::<Option<String>>(31)
            .map_err(|_| HostError::Journal)?
            .is_some()
        && row
            .get::<Option<String>>(32)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && row.get::<i64>(33).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(34).map_err(|_| HostError::Journal)? == 0)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
