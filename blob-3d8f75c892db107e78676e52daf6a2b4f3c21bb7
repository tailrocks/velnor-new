//! Transaction and row decoding helpers for the file-backed journal.

use crate::error::HostError;
#[cfg(test)]
use crate::journal::IntentState;
#[cfg(test)]
use crate::reconcile::IntentRow;

#[cfg(test)]
pub(super) fn intent_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let state_text: String = row.get(3).map_err(|_| HostError::Journal)?;
    Ok(IntentRow {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        kind: row.get(1).map_err(|_| HostError::Journal)?,
        subject: row.get(2).map_err(|_| HostError::Journal)?,
        state: IntentState::parse(&state_text)?,
        docker_id: row.get(4).map_err(|_| HostError::Journal)?,
        dind_id: row.get(5).map_err(|_| HostError::Journal)?,
        github_runner_id: row.get(6).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(7).map_err(|_| HostError::Journal)?,
        launch_id: row.get(8).map_err(|_| HostError::Journal)?,
        assignment_key: row.get(9).map_err(|_| HostError::Journal)?,
        seed_generation_id: row.get(10).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(11).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(12).map_err(|_| HostError::Journal)?,
        acquired: row.get(13).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(14).map_err(|_| HostError::Journal)?,
        runner_completed: row.get(15).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(16).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(17).map_err(|_| HostError::Journal)?,
        request_id: row.get(18).map_err(|_| HostError::Journal)?,
        runner_name: row.get(19).map_err(|_| HostError::Journal)?,
        docker_engine_id: row.get(20).map_err(|_| HostError::Journal)?,
        launch_phase: row
            .get::<Option<String>>(21)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            .map(crate::reconcile::LaunchPhase::parse)
            .transpose()?,
    })
}

pub(super) fn one_row(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) fn token_rejected(token: &str) -> bool {
    token.is_empty() || token.chars().any(|ch| matches!(ch, '\'' | '"'))
}
