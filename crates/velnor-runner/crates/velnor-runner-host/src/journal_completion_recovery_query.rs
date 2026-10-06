//! Bounded selection and exact decoding for recovery leases.

use crate::error::HostError;
use crate::journal::intent_row;
use crate::reconcile::IntentRow;

use super::super::{
    RecoveryIdentity, RecoveryLease, assigned_request, is_message_name, is_session_name,
};

pub(super) async fn due_recovery_ids(
    connection: &turso::Connection,
    scale_set_id: i64,
    now: i64,
    limit: u32,
) -> Result<Vec<i64>, HostError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut rows = connection
        .query(
            "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.github_runner_id, i.cleanup_proven, i.dind_id, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.docker_engine_id, i.launch_phase, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed FROM intents AS i LEFT JOIN launch_recovery AS r ON r.intent_id = i.id WHERE i.kind = 'launch' AND i.cleanup_proven = 0 AND i.scale_set_id = ?1 AND i.runner_name IS NOT NULL AND i.worker_volume IS NOT NULL AND COALESCE(r.retry_after, 0) <= ?2 AND COALESCE(r.lease_until, 0) <= ?2 AND NOT EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = i.id) ORDER BY COALESCE(r.retry_after, 0), i.id",
            (scale_set_id, now),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut ids = Vec::with_capacity(limit as usize);
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let intent = intent_row(&row)?;
        if recoverable(&intent, &recovery_identity(&row)?) {
            ids.push(intent.id);
            if ids.len() == limit as usize {
                break;
            }
        }
    }
    Ok(ids)
}

pub(super) async fn claim_recovery_row(
    connection: &turso::Connection,
    id: i64,
    now: i64,
    lease_until: i64,
) -> Result<Option<RecoveryLease>, HostError> {
    connection
        .execute(
            "INSERT OR IGNORE INTO launch_recovery (intent_id) VALUES (?1)",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT generation, attempts, retry_after, lease_until FROM launch_recovery WHERE intent_id = ?1",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let generation: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let attempts: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let retry_after: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let current_lease: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    if [generation, attempts, retry_after, current_lease]
        .iter()
        .any(|value| *value < 0)
    {
        return Err(HostError::Journal);
    }
    if retry_after > now || current_lease > now {
        return Ok(None);
    }
    let next_generation = generation.checked_add(1).ok_or(HostError::Journal)?;
    let next_attempts = attempts.saturating_add(1).min(super::MAX_RECOVERY_ATTEMPTS);
    let changed = connection
        .execute(
            "UPDATE launch_recovery SET generation = ?1, attempts = ?2, lease_until = ?3 WHERE intent_id = ?4 AND generation = ?5 AND retry_after <= ?6 AND lease_until <= ?6",
            (next_generation, next_attempts, lease_until, id, generation, now),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(Some(
            load_recovery_lease(connection, id)
                .await?
                .ok_or(HostError::Journal)?,
        ))
    } else {
        Ok(None)
    }
}

pub(super) async fn load_recovery_lease(
    connection: &turso::Connection,
    id: i64,
) -> Result<Option<RecoveryLease>, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.github_runner_id, i.cleanup_proven, i.dind_id, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.docker_engine_id, i.launch_phase, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed, r.generation, r.attempts, r.lease_until FROM intents AS i JOIN launch_recovery AS r ON r.intent_id = i.id WHERE i.id = ?1 AND i.kind = 'launch' AND i.cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = i.id)",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(RecoveryLease {
        intent: intent_row(&row)?,
        identity: recovery_identity(&row)?,
        generation: row.get(22).map_err(|_| HostError::Journal)?,
        attempts: u32::try_from(row.get::<i64>(23).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?,
        lease_until: row.get(24).map_err(|_| HostError::Journal)?,
    }))
}

pub(super) async fn current_recovery_claim(
    connection: &turso::Connection,
    lease: &RecoveryLease,
    now: i64,
) -> Result<bool, HostError> {
    let Some(current) = load_recovery_lease(connection, lease.intent.id).await? else {
        return Ok(false);
    };
    Ok(&current == lease && current.lease_until > now)
}

fn recovery_identity(row: &turso::Row) -> Result<RecoveryIdentity, HostError> {
    Ok(RecoveryIdentity {
        scale_set_id: row.get(9).map_err(|_| HostError::Journal)?,
        runner_request_id: row.get(10).map_err(|_| HostError::Journal)?,
        runner_name: row.get(11).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(17).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(18).map_err(|_| HostError::Journal)?,
        acquired: row.get(19).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(20).map_err(|_| HostError::Journal)?,
    })
}

fn recoverable(intent: &IntentRow, identity: &RecoveryIdentity) -> bool {
    if intent.cleanup_proven
        || intent.kind != "launch"
        || intent
            .worker_volume
            .as_deref()
            .is_none_or(|worker| !valid_worker(worker))
        || identity.scale_set_id <= 0
        || !valid_runner_identity(&intent.subject, identity)
    {
        return false;
    }
    match identity.runner_request_id {
        Some(_) => {
            identity.acquire_attempted
                && identity.acquire_resolved
                && identity.acquired
                && identity.jit_requested
        }
        None => !identity.acquire_attempted && !identity.acquire_resolved && !identity.acquired,
    }
}

fn valid_runner_identity(subject: &str, identity: &RecoveryIdentity) -> bool {
    let name = match identity.runner_request_id {
        Some(request) if request > 0 && assigned_request(subject) == Some(request) => {
            format!("v{request}")
        }
        Some(_) => return false,
        None if is_session_name(subject) || is_message_name(subject) => subject.to_owned(),
        None => return false,
    };
    identity.runner_name == name
}

fn valid_worker(worker: &str) -> bool {
    worker.len() == 33
        && worker.starts_with('w')
        && worker[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
