use crate::error::HostError;

#[path = "terminal/post_jit.rs"]
mod post_jit;
pub(in crate::journal) use post_jit::validate_retirement_digest;

pub(super) async fn settle_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
    reservation_id: i64,
) -> Result<(), HostError> {
    if post_jit::settle_if_claimed(connection, connection_id, intent_id, reservation_id).await? {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT 1 FROM physical_attempt_current WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let has_current_attempt = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let completion_generation = if has_current_attempt {
        Some(
            active_completion_generation(connection, connection_id, intent_id)
                .await?
                .ok_or(HostError::Journal)?,
        )
    } else {
        None
    };
    if has_current_attempt
        && !crate::journal::connection::record_terminal_cleanup_outcome(
            connection,
            connection_id,
            reservation_id,
            intent_id,
        )
        .await?
    {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET state = 'released' WHERE id = ?1 AND connection_id = ?2 AND remote_cleanup_proven = 1 AND local_cleanup_proven = 1 AND state != 'released'",
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    if has_current_attempt {
        let retired = connection
            .execute(
                "DELETE FROM physical_attempt_current WHERE reservation_id = ?1 AND intent_id = ?2",
                (reservation_id, intent_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if retired != 1 {
            return Err(HostError::Journal);
        }
        retire_completed_claim(
            connection,
            connection_id,
            intent_id,
            reservation_id,
            completion_generation.ok_or(HostError::Journal)?,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn active_completion_generation(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT cleanup.claim_generation FROM connection_completion_cleanup AS cleanup JOIN connection_intents AS intent ON intent.id = cleanup.intent_id AND intent.connection_id = cleanup.connection_id WHERE cleanup.intent_id = ?1 AND cleanup.connection_id = ?2 AND cleanup.scale_set_id = intent.scale_set_id AND cleanup.runner_absent = 1 AND cleanup.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND intent.kind = 'launch' AND intent.cleanup_proven = 1",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let generation: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || generation <= 0 {
        return Err(HostError::Journal);
    }
    Ok(Some(generation))
}

pub(super) async fn retire_completed_claim(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
    reservation_id: i64,
    generation: i64,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_completion_cleanup SET retry_after = 0, lease_until = 0 WHERE intent_id = ?1 AND connection_id = ?2 AND claim_generation = ?3 AND runner_absent = 1 AND lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND EXISTS (SELECT 1 FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND cleanup_proven = 1) AND EXISTS (SELECT 1 FROM capacity_reservations AS reservation JOIN physical_attempts AS attempt ON attempt.reservation_id = reservation.id AND attempt.intent_id = reservation.intent_id AND attempt.connection_id = reservation.connection_id JOIN physical_attempt_outcomes AS outcome ON outcome.lineage_id = attempt.lineage_id AND outcome.generation = attempt.generation AND outcome.reservation_id = reservation.id WHERE reservation.id = ?4 AND reservation.intent_id = ?1 AND reservation.connection_id = ?2 AND reservation.state = 'released' AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND outcome.outcome = 'normal_completion' AND NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current WHERE current.reservation_id = reservation.id))",
            (intent_id, connection_id, generation, reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
