use crate::error::HostError;

use super::super::claim::JitConflictRecoveryClaim;
use super::Target;

pub(in crate::journal::connection::reservation::jit_conflict) async fn has_delete_started(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND phase_index=3 AND phase='delete_started'",
            [claim.reservation_id()],
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
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || count < 0 {
        return Err(HostError::Journal);
    }
    Ok(count > 0)
}

pub(in crate::journal::connection::reservation::jit_conflict) async fn latest_current_observation(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
) -> Result<Target, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_id,runner_name,runner_scale_set_id,phase FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=2",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let target = Target {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        name: row.get(1).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
    };
    let phase: String = row.get(3).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || target.name != claim.requested_name()
        || target.scale_set_id != claim.scale_set_id()
        || !matches!(phase.as_str(), "remote_target" | "remote_initial_absent")
        || (phase == "remote_target" && target.id.is_none_or(|id| id <= 0))
        || (phase == "remote_initial_absent" && target.id.is_some())
    {
        return Err(HostError::Identity);
    }
    Ok(target)
}

pub(in crate::journal::connection::reservation::jit_conflict) async fn current_observation(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
) -> Result<Option<Target>, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_id,runner_name,runner_scale_set_id,phase FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=2",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let target = Target {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        name: row.get(1).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
    };
    let phase: String = row.get(3).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || target.name != claim.requested_name()
        || target.scale_set_id != claim.scale_set_id()
        || !matches!(phase.as_str(), "remote_target" | "remote_initial_absent")
        || (phase == "remote_target" && target.id.is_none_or(|id| id <= 0))
        || (phase == "remote_initial_absent" && target.id.is_some())
    {
        return Err(HostError::Identity);
    }
    Ok(Some(target))
}

pub(in crate::journal::connection::reservation::jit_conflict) async fn latest_delete_target(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
) -> Result<Target, HostError> {
    let mut rows = connection
        .query(
            "SELECT deletion.runner_id,deletion.runner_name,deletion.runner_scale_set_id FROM physical_attempt_jit_conflict_recovery_progress deletion JOIN physical_attempt_jit_conflict_recovery_progress target ON target.reservation_id=deletion.reservation_id AND target.claim_generation=deletion.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=deletion.runner_id AND target.runner_name=deletion.runner_name AND target.runner_scale_set_id=deletion.runner_scale_set_id WHERE deletion.reservation_id=?1 AND deletion.phase_index=3 AND deletion.phase='delete_started' ORDER BY deletion.claim_generation DESC LIMIT 1",
            [claim.reservation_id()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let target = Target {
        id: Some(row.get(0).map_err(|_| HostError::Journal)?),
        name: row.get(1).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || target.id.is_none_or(|id| id <= 0)
        || target.name != claim.requested_name()
        || target.scale_set_id != claim.scale_set_id()
    {
        return Err(HostError::Identity);
    }
    Ok(target)
}
